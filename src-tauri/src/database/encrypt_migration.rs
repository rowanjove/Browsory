use chrono::Utc;
use rusqlite::{Connection, OpenFlags};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use tracing::{error, info};

use crate::database::migrations::run_migrations;
use crate::error::{AppError, AppResult};

pub const SQLITE_HEADER_MAGIC: &[u8; 16] = b"SQLite format 3\0";

/// Checks if a file is an unencrypted plaintext SQLite database by inspecting its header magic bytes.
pub fn is_plain_sqlite_database(db_path: &Path) -> bool {
    if !db_path.exists() {
        return false;
    }
    let mut file = match File::open(db_path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut header = [0u8; 16];
    if file.read_exact(&mut header).is_err() {
        return false;
    }
    &header == SQLITE_HEADER_MAGIC
}

/// Verifies SQLite database integrity via PRAGMA integrity_check
pub fn verify_sqlite_integrity(db_path: &Path) -> AppResult<bool> {
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let result: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    Ok(result.to_lowercase() == "ok")
}

#[derive(Debug, Clone, Default)]
pub struct TableRecordCounts {
    pub sources: i64,
    pub urls: i64,
    pub visits: i64,
    pub tags: i64,
    pub favorites: i64,
    pub settings: i64,
}

impl TableRecordCounts {
    pub fn collect_from(conn: &Connection) -> Self {
        let count_table = |table: &str| -> i64 {
            conn.query_row(&format!("SELECT COUNT(*) FROM {}", table), [], |r| r.get(0))
                .unwrap_or(0)
        };

        TableRecordCounts {
            sources: count_table("sources"),
            urls: count_table("urls"),
            visits: count_table("visits"),
            tags: count_table("tags"),
            favorites: count_table("favorites"),
            settings: count_table("settings"),
        }
    }

    pub fn matches(&self, other: &TableRecordCounts) -> bool {
        self.sources == other.sources
            && self.urls == other.urls
            && self.visits == other.visits
            && self.tags == other.tags
            && self.favorites == other.favorites
            && self.settings == other.settings
    }
}

/// Executes a safe, multi-phase schema migration of an existing archive.db.
///
/// This routine does not encrypt SQLite pages; it only recreates the current
/// schema and preserves known application tables before an atomic swap. A
/// future SQLCipher implementation must replace this routine rather than
/// treating a normal SQLite file as encrypted.
pub fn migrate_database_safely(source_db_path: &Path, backups_dir: &Path) -> AppResult<PathBuf> {
    if !source_db_path.exists() {
        return Err(AppError::System("Source database does not exist".into()));
    }

    info!(
        "Starting database migration safety verification for: {:?}",
        source_db_path
    );

    // Phase 1: Verify source database integrity
    if !verify_sqlite_integrity(source_db_path)? {
        return Err(AppError::Database(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CORRUPT),
            Some("源数据库未通过 integrity_check 校验，中止迁移以防数据损坏".into()),
        )));
    }

    // Phase 2: Create pre-migration backup
    let _ = fs::create_dir_all(backups_dir);
    let now_str = Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let pre_backup_name = format!("archive_pre_migration_{}.db", now_str);
    let pre_backup_path = backups_dir.join(&pre_backup_name);

    info!(
        "Creating safety pre-migration backup at {:?}",
        pre_backup_path
    );
    fs::copy(source_db_path, &pre_backup_path)?;

    // Also copy WAL and SHM if present
    let wal_path = source_db_path.with_file_name("archive.db-wal");
    if wal_path.exists() {
        let _ = fs::copy(
            &wal_path,
            backups_dir.join(format!("archive_pre_migration_{}.db-wal", now_str)),
        );
    }

    // Phase 3: Connect to source and capture baseline counts
    let src_conn = Connection::open_with_flags(source_db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let src_counts = TableRecordCounts::collect_from(&src_conn);
    drop(src_conn); // Explicitly close connection so Windows allows atomic file rename
    info!("Source database record baseline: {:?}", src_counts);

    // Phase 4: Create fresh staging database
    let staging_db_path = source_db_path.with_file_name("archive.staging.db");
    if staging_db_path.exists() {
        let _ = fs::remove_file(&staging_db_path);
    }

    let migration_result = (|| -> AppResult<()> {
        let mut dest_conn = Connection::open(&staging_db_path)?;

        // Apply high performance pragmas
        let _: Result<String, _> =
            dest_conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0));
        let _: Result<String, _> =
            dest_conn.query_row("PRAGMA synchronous = NORMAL", [], |r| r.get(0));
        let _: Result<String, _> =
            dest_conn.query_row("PRAGMA foreign_keys = OFF", [], |r| r.get(0));

        // Run migrations to establish schema
        run_migrations(&mut dest_conn)?;

        // Attach source database to stream records cleanly inside SQLite
        let attach_sql = format!(
            "ATTACH DATABASE '{}' AS src_db",
            source_db_path.to_string_lossy().replace('\'', "''")
        );
        dest_conn.execute_batch(&attach_sql)?;

        let tx = dest_conn.transaction()?;

        // Stream data table by table
        tx.execute("INSERT INTO main.sources SELECT * FROM src_db.sources", [])?;
        tx.execute("INSERT INTO main.urls SELECT * FROM src_db.urls", [])?;
        tx.execute("INSERT INTO main.visits SELECT * FROM src_db.visits", [])?;

        let has_src_tags: i64 = tx
            .query_row(
                "SELECT count(*) FROM src_db.sqlite_master WHERE type='table' AND name='tags'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if has_src_tags > 0 {
            tx.execute(
                "INSERT OR IGNORE INTO main.tags SELECT * FROM src_db.tags",
                [],
            )?;
            tx.execute(
                "INSERT OR IGNORE INTO main.url_tags SELECT * FROM src_db.url_tags",
                [],
            )?;
            tx.execute(
                "INSERT OR IGNORE INTO main.favorites SELECT * FROM src_db.favorites",
                [],
            )?;
        }

        let has_src_rules: i64 = tx.query_row(
            "SELECT count(*) FROM src_db.sqlite_master WHERE type='table' AND name='privacy_rules'",
            [],
            |r| r.get(0),
        ).unwrap_or(0);
        if has_src_rules > 0 {
            tx.execute(
                "INSERT OR IGNORE INTO main.privacy_rules SELECT * FROM src_db.privacy_rules",
                [],
            )?;
        }

        let has_src_envelope: i64 = tx.query_row(
            "SELECT count(*) FROM src_db.sqlite_master WHERE type='table' AND name='master_key_envelope'",
            [],
            |r| r.get(0),
        ).unwrap_or(0);
        if has_src_envelope > 0 {
            tx.execute("INSERT OR REPLACE INTO main.master_key_envelope SELECT * FROM src_db.master_key_envelope", [])?;
        }

        tx.execute(
            "INSERT OR REPLACE INTO main.settings SELECT * FROM src_db.settings",
            [],
        )?;

        // Preserve tables added by later migrations. The source schema is
        // user data, so silently omitting one of these tables is unacceptable.
        for table in [
            "app_security",
            "backups",
            "master_key_envelope",
            "smart_collections",
            "page_embeddings",
            "topics",
            "url_topics",
            "link_health",
            "page_contents",
            "offline_archives",
            "background_jobs",
        ] {
            let source_exists: i64 = tx.query_row(
                "SELECT COUNT(*) FROM src_db.sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |r| r.get(0),
            )?;
            if source_exists == 0 {
                continue;
            }
            let destination_exists: i64 = tx.query_row(
                "SELECT COUNT(*) FROM main.sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |r| r.get(0),
            )?;
            if destination_exists == 0 {
                return Err(AppError::Migration(format!(
                    "目标 schema 缺少源表 {}，已中止迁移以防数据丢失",
                    table
                )));
            }
            tx.execute(
                &format!(
                    "INSERT OR REPLACE INTO main.\"{table}\" SELECT * FROM src_db.\"{table}\""
                ),
                [],
            )?;
        }

        tx.commit()?;

        dest_conn.execute_batch("DETACH DATABASE src_db;")?;

        // Rebuild FTS5 index
        dest_conn.execute_batch("INSERT INTO urls_fts(urls_fts) VALUES('rebuild');")?;

        // Re-enable foreign keys
        dest_conn.execute_batch("PRAGMA foreign_keys = ON;")?;

        // Phase 5: Verification of staging database
        let staging_integrity: String =
            dest_conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        if staging_integrity.to_lowercase() != "ok" {
            return Err(AppError::System(format!(
                "Staging database integrity check failed: {}",
                staging_integrity
            )));
        }

        let dest_counts = TableRecordCounts::collect_from(&dest_conn);
        info!("Staging database verified records: {:?}", dest_counts);

        if !src_counts.matches(&dest_counts) {
            return Err(AppError::System(format!(
                "Record count mismatch during migration: Source={:?}, Dest={:?}",
                src_counts, dest_counts
            )));
        }

        Ok(())
    })();

    if let Err(e) = migration_result {
        error!(
            "Database migration failed: {}. Cleaning staging file and preserving original.",
            e
        );
        if staging_db_path.exists() {
            let _ = fs::remove_file(&staging_db_path);
        }
        return Err(e);
    }

    // Phase 6: Atomic swap
    let legacy_plaintext_path =
        source_db_path.with_file_name(format!("archive.db.plaintext_v0_{}.bak", now_str));
    info!(
        "Renaming original database to legacy backup: {:?}",
        legacy_plaintext_path
    );
    fs::rename(source_db_path, &legacy_plaintext_path)?;

    // Remove old wal/shm if present
    if wal_path.exists() {
        let _ = fs::remove_file(&wal_path);
    }
    let shm_path = source_db_path.with_file_name("archive.db-shm");
    if shm_path.exists() {
        let _ = fs::remove_file(&shm_path);
    }

    info!("Swapping staging database to active archive.db");
    fs::rename(&staging_db_path, source_db_path)?;

    info!("Database migration completed successfully with zero data loss!");
    Ok(source_db_path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sqlite_header_magic_detection() {
        let temp_dir = std::env::temp_dir().join(format!(
            "browsory_test_header_{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let _ = fs::create_dir_all(&temp_dir);
        let db_path = temp_dir.join("test.db");

        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute("CREATE TABLE t (id INTEGER PRIMARY KEY)", [])
                .unwrap();
        }

        assert!(is_plain_sqlite_database(&db_path));
        assert!(verify_sqlite_integrity(&db_path).unwrap());

        // Corrupted file test
        let non_db_path = temp_dir.join("corrupt.db");
        fs::write(&non_db_path, b"NOT_SQLITE_HEADER").unwrap();
        assert!(!is_plain_sqlite_database(&non_db_path));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_migrate_database_safely_roundtrip() {
        let temp_dir = std::env::temp_dir().join(format!(
            "browsory_test_mig_{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let _ = fs::create_dir_all(&temp_dir);
        let src_db_path = temp_dir.join("archive.db");
        let backups_dir = temp_dir.join("backups");

        // 1. Populate a legacy database
        {
            let mut conn = Connection::open(&src_db_path).unwrap();
            run_migrations(&mut conn).unwrap();

            conn.execute(
                "INSERT INTO sources (browser, profile, source_type, history_path) VALUES ('chrome', 'Default', 'sqlite', 'mock/path')",
                [],
            ).unwrap();

            conn.execute(
                "INSERT INTO urls (url, title, domain, normalized_url) VALUES ('https://browsory.app', 'Browsory Home', 'browsory.app', 'https://browsory.app')",
                [],
            ).unwrap();

            conn.execute(
                "INSERT INTO visits (source_id, url_id, source_visit_id, visit_time, transition, from_visit, visit_duration, event_hash, imported_at) VALUES (1, 1, 1001, 1700000000000, 0, 0, 10, 'hash123', 1700000000000)",
                [],
            ).unwrap();
        }

        // 2. Perform safe migration
        let migrated_path = migrate_database_safely(&src_db_path, &backups_dir).unwrap();
        assert_eq!(migrated_path, src_db_path);

        // 3. Verify target database has exact records and integrity
        {
            let conn = Connection::open(&src_db_path).unwrap();
            let counts = TableRecordCounts::collect_from(&conn);
            assert_eq!(counts.sources, 1);
            assert_eq!(counts.urls, 1);
            assert_eq!(counts.visits, 1);

            // Verify FTS works on migrated records
            let fts_title: String = conn
                .query_row(
                    "SELECT title FROM urls_fts WHERE urls_fts MATCH 'Browsory'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(fts_title, "Browsory Home");
        }

        // 4. Verify safety backup exists
        let backup_files: Vec<_> = fs::read_dir(&backups_dir).unwrap().flatten().collect();
        assert!(!backup_files.is_empty());

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
