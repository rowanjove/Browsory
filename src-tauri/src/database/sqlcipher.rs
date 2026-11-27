use rusqlite::{Connection, OpenFlags};
use std::path::Path;
use std::time::Duration;
use tracing::info;
use zeroize::Zeroizing;

use crate::error::{AppError, AppResult};

pub(crate) fn register_privacy_functions(conn: &Connection) -> AppResult<()> {
    use rusqlite::functions::FunctionFlags;
    conn.create_scalar_function(
        "browsory_matches_privacy_rule",
        3,
        FunctionFlags::SQLITE_UTF8
            | FunctionFlags::SQLITE_DETERMINISTIC
            | FunctionFlags::SQLITE_INNOCUOUS,
        |context| {
            Ok(crate::database::repository::matches_privacy_rule(
                &context.get::<String>(0)?,
                &context.get::<String>(1)?,
                &context.get::<String>(2)?,
            ))
        },
    )?;
    conn.create_scalar_function(
        "browsory_non_public_url",
        1,
        FunctionFlags::SQLITE_UTF8
            | FunctionFlags::SQLITE_DETERMINISTIC
            | FunctionFlags::SQLITE_INNOCUOUS,
        |context| {
            Ok(crate::history::health::is_non_public_url(
                &context.get::<String>(0)?,
            ))
        },
    )?;
    Ok(())
}

/// Apply before queries on every connection that handles private data. This
/// avoids SQLite sorter/temp-table spill files, not OS paging or crash dumps.
pub fn configure_private_connection(conn: &Connection) -> AppResult<()> {
    register_privacy_functions(conn)?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    let mode: i64 = conn.pragma_query_value(None, "temp_store", |row| row.get(0))?;
    if mode != 2 {
        return Err(AppError::Security(
            "无法启用 SQLite 内存临时存储，已中止操作".into(),
        ));
    }
    // SQLCipher can return a status row for this setting. Consume it rather
    // than relying on execute_batch, which rejects rows with extra_check.
    let mut statement = conn.prepare("PRAGMA cipher_memory_security = ON")?;
    let mut rows = statement.query([])?;
    while rows.next()?.is_some() {}
    drop(rows);
    drop(statement);
    let enabled: String =
        conn.pragma_query_value(None, "cipher_memory_security", |row| row.get(0))?;
    if enabled != "1" {
        return Err(AppError::Security(
            "无法启用 SQLCipher 内存释放清理，已中止操作".into(),
        ));
    }
    Ok(())
}

/// Verifies that SQLCipher is genuinely compiled and linked.
/// Plain SQLite ignores `PRAGMA cipher_version` and returns no rows.
pub fn verify_sqlcipher_support(conn: &Connection) -> AppResult<String> {
    let mut stmt = conn
        .prepare("PRAGMA cipher_version;")
        .map_err(|e| AppError::Security(format!("无法执行 PRAGMA cipher_version: {}", e)))?;

    let version_opt: Option<String> = stmt
        .query_row([], |row| row.get(0))
        .map(Some)
        .unwrap_or(None);

    match version_opt {
        Some(v) if !v.trim().is_empty() => Ok(v),
        _ => Err(AppError::Security(
            "SQLCipher 扩展未启用！普通 SQLite 无法提供数据库静态加密保证。".into(),
        )),
    }
}

/// Applies a 256-bit raw key to the SQLite connection using SQLCipher's raw key format (x'<hex>').
pub fn apply_raw_key(conn: &Connection, key: &[u8; 32]) -> AppResult<()> {
    let key_hex = Zeroizing::new(hex::encode(key));
    let pragma = Zeroizing::new(format!("PRAGMA key = \"x'{}'\";", key_hex.as_str()));
    // Some SQLCipher builds return a status row for PRAGMA key. Consume any
    // results instead of executing the same secret-bearing statement twice.
    let apply = || -> rusqlite::Result<()> {
        let mut statement = conn.prepare(pragma.as_str())?;
        let mut rows = statement.query([])?;
        while rows.next()?.is_some() {}
        Ok(())
    };
    apply().map_err(|_| AppError::Security("设置 SQLCipher 数据库密钥失败".into()))?;

    Ok(())
}

const DEFAULT_PRIMARY_CACHE_KIB: i64 = -65_536;
const DEFAULT_READ_CACHE_KIB: i64 = -32_768;
const DEFAULT_WAL_SIZE_LIMIT_BYTES: i64 = 33_554_432;

fn open_verified_encrypted_connection(
    db_path: &Path,
    master_key: &[u8; 32],
    flags: OpenFlags,
) -> AppResult<Connection> {
    let conn = Connection::open_with_flags(db_path, flags)?;
    configure_private_connection(&conn)?;

    // 1. Verify SQLCipher engine exists
    let cipher_ver = verify_sqlcipher_support(&conn)?;
    info!("SQLCipher engine verified: v{}", cipher_ver);

    // 2. Apply 32-byte raw master key
    apply_raw_key(&conn, master_key)?;

    // 3. Probe database header with sqlite_master to verify key validity immediately.
    // In SQLCipher, incorrect key or corrupt database will fail here with SQLITE_NOTADB.
    let probe_result: Result<i64, rusqlite::Error> =
        conn.query_row("SELECT count(*) FROM sqlite_master;", [], |r| r.get(0));

    if let Err(e) = probe_result {
        return Err(AppError::Security(format!(
            "数据库解密校验失败 (密钥错误或文件非合法 SQLCipher 加密库): {}",
            e
        )));
    }

    conn.busy_timeout(Duration::from_millis(5000))?;
    Ok(conn)
}

/// Opens an existing encrypted database connection or creates a new one with the given raw 32-byte master key.
/// Validates key correctness immediately via sqlite_master probe before returning.
pub fn open_encrypted_database(
    db_path: &Path,
    master_key: &[u8; 32],
    create_if_missing: bool,
) -> AppResult<Connection> {
    open_encrypted_database_with_cache(
        db_path,
        master_key,
        create_if_missing,
        DEFAULT_PRIMARY_CACHE_KIB,
    )
}

pub fn open_encrypted_database_with_cache(
    db_path: &Path,
    master_key: &[u8; 32],
    create_if_missing: bool,
    cache_kib: i64,
) -> AppResult<Connection> {
    if !create_if_missing && !db_path.exists() {
        return Err(AppError::System(format!(
            "数据库文件不存在且未启用创建模式: {:?}",
            db_path
        )));
    }

    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let flags = if create_if_missing {
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    };

    let conn = open_verified_encrypted_connection(db_path, master_key, flags)?;

    // 4. Standard secure SQLite configurations + bounded WAL & page cache for large archives
    let _journal_mode: String = conn.query_row("PRAGMA journal_mode = WAL;", [], |r| r.get(0))?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "cache_size", cache_kib)?;
    let _wal_limit: i64 = conn.query_row(
        &format!(
            "PRAGMA journal_size_limit = {};",
            DEFAULT_WAL_SIZE_LIMIT_BYTES
        ),
        [],
        |r| r.get(0),
    )?;

    Ok(conn)
}

/// Opens an existing SQLCipher database as a concurrent read-only WAL reader.
pub fn open_encrypted_database_read_only(
    db_path: &Path,
    master_key: &[u8; 32],
) -> AppResult<Connection> {
    if !db_path.exists() {
        return Err(AppError::System(format!(
            "只读连接目标数据库文件不存在: {:?}",
            db_path
        )));
    }
    let conn =
        open_verified_encrypted_connection(db_path, master_key, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.pragma_update(None, "query_only", true)?;
    conn.pragma_update(None, "cache_size", DEFAULT_READ_CACHE_KIB)?;
    Ok(conn)
}

/// Checks both SQLCipher page authentication and SQLite logical integrity.
pub fn check_encrypted_integrity(conn: &Connection) -> AppResult<bool> {
    verify_sqlcipher_support(conn)?;
    let mut cipher = conn.prepare("PRAGMA cipher_integrity_check;")?;
    // SQLCipher returns zero rows on success, one row per error otherwise.
    if cipher.query([])?.next()?.is_some() {
        return Ok(false);
    }
    let mut statement = conn.prepare("PRAGMA integrity_check;")?;
    let mut rows = statement.query([])?;
    let mut found_ok = false;
    while let Some(row) = rows.next()? {
        let result: String = row.get(0)?;
        if result.trim() != "ok" {
            return Ok(false);
        }
        found_ok = true;
    }
    Ok(found_ok)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::RngCore;
    use std::fs;

    #[test]
    fn private_connections_keep_sorters_and_temp_tables_in_memory() {
        let conn = Connection::open_in_memory().unwrap();
        configure_private_connection(&conn).unwrap();
        conn.execute_batch("CREATE TEMP TABLE private_sort(value TEXT); INSERT INTO private_sort VALUES('private text'),('other');").unwrap();
        assert_eq!(
            conn.pragma_query_value(None, "temp_store", |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            conn.pragma_query_value(None, "cipher_memory_security", |r| r.get::<_, String>(0))
                .unwrap(),
            "1"
        );
        let file: String = conn
            .query_row(
                "SELECT file FROM pragma_database_list WHERE name='temp'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(file.is_empty());
        assert_eq!(
            conn.query_row(
                "SELECT value FROM private_sort ORDER BY value DESC LIMIT 1",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "private text"
        );
    }

    fn generate_test_key() -> [u8; 32] {
        let mut key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut key);
        key
    }

    #[test]
    fn test_sqlcipher_creation_and_reopen_lifecycle() {
        let temp_dir =
            std::env::temp_dir().join(format!("browsory_sqlcipher_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test_encrypted.db");

        let key1 = generate_test_key();
        let key2 = generate_test_key();

        // 1. Create encrypted database
        {
            let conn = open_encrypted_database(&db_path, &key1, true).expect("Open encrypted DB");
            assert!(check_encrypted_integrity(&conn).expect("Integrity check"));

            // Verify FTS5 works
            conn.execute_batch(
                "CREATE TABLE items (id INTEGER PRIMARY KEY, content TEXT);
                 CREATE VIRTUAL TABLE items_fts USING fts5(content);
                 INSERT INTO items (id, content) VALUES (1, 'Browsory Rust Security Vault');
                 INSERT INTO items_fts (rowid, content) VALUES (1, 'Browsory Rust Security Vault');",
            )
            .expect("Create schema and FTS5 table");
            let journal: String = conn
                .pragma_query_value(None, "journal_mode", |r| r.get(0))
                .unwrap();
            let foreign_keys: i64 = conn
                .pragma_query_value(None, "foreign_keys", |r| r.get(0))
                .unwrap();
            assert_eq!(journal, "wal");
            assert_eq!(foreign_keys, 1);
            for path in [&db_path, &db_path.with_extension("db-wal")] {
                let bytes = fs::read(path).unwrap();
                assert!(!bytes
                    .windows(b"Browsory Rust Security Vault".len())
                    .any(|part| part == b"Browsory Rust Security Vault"));
            }

            // Query FTS5
            let count: i64 = conn
                .query_row(
                    "SELECT count(*) FROM items_fts WHERE items_fts MATCH 'Security';",
                    [],
                    |r| r.get(0),
                )
                .expect("Query FTS5");
            assert_eq!(count, 1);
        }

        // 2. Reopening with correct key succeeds
        {
            let conn =
                open_encrypted_database(&db_path, &key1, false).expect("Reopen with correct key");
            let item: String = conn
                .query_row("SELECT content FROM items WHERE id = 1;", [], |r| r.get(0))
                .expect("Read item");
            assert_eq!(item, "Browsory Rust Security Vault");
            assert!(check_encrypted_integrity(&conn).expect("Integrity check on reopen"));
        }

        // 3. Reopening with wrong key fails with security error
        {
            let res = open_encrypted_database(&db_path, &key2, false);
            assert!(res.is_err(), "Opening with wrong key must fail");
        }

        // 4. Plain SQLite connection without key fails to read the encrypted file
        {
            let plain_conn =
                Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                    .expect("Open raw file");
            let read_attempt =
                plain_conn.query_row("SELECT count(*) FROM items;", [], |r| r.get::<_, i64>(0));
            assert!(
                read_attempt.is_err(),
                "Plain SQLite must not read encrypted pages"
            );
        }

        // 5. Test VACUUM INTO on encrypted connection creates an identical encrypted copy with same key
        {
            let conn =
                open_encrypted_database(&db_path, &key1, false).expect("Reopen with correct key");
            let vacuum_path = temp_dir.join("backup.db");
            let vacuum_sql = format!(
                "VACUUM INTO '{}';",
                vacuum_path.to_str().unwrap().replace('\\', "/")
            );
            conn.execute_batch(&vacuum_sql)
                .expect("Execute VACUUM INTO");
            assert!(vacuum_path.exists());

            // Opening vacuumed backup with key1 succeeds
            let vacuum_conn = open_encrypted_database(&vacuum_path, &key1, false)
                .expect("Open vacuumed DB with key1");
            let item: String = vacuum_conn
                .query_row("SELECT content FROM items WHERE id = 1;", [], |r| r.get(0))
                .expect("Read item from vacuumed DB");
            assert_eq!(item, "Browsory Rust Security Vault");

            // Opening vacuumed backup with wrong key fails
            let fail_attempt = open_encrypted_database(&vacuum_path, &key2, false);
            assert!(
                fail_attempt.is_err(),
                "Opening vacuumed backup with wrong key must fail"
            );
        }

        // 6. Test read-only WAL connection reads concurrently and rejects writes
        {
            let ro_conn = open_encrypted_database_read_only(&db_path, &key1)
                .expect("Open read-only encrypted DB");
            let item: String = ro_conn
                .query_row("SELECT content FROM items WHERE id = 1;", [], |r| r.get(0))
                .expect("Read item via read-only connection");
            assert_eq!(item, "Browsory Rust Security Vault");
            assert!(
                ro_conn
                    .execute("INSERT INTO items (id, content) VALUES (2, 'blocked')", [])
                    .is_err(),
                "Read-only connection must reject writes"
            );
            assert!(open_encrypted_database_read_only(&db_path, &key2).is_err());
        }

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }
}
