use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::time::Instant;
use tracing::{error, info};

use super::dedup::calculate_event_hash;
use super::normalize::normalize_url;
use super::parser::{read_chromium_history_snapshot, RawVisitRecord};
use super::record_spool::{CancelProbe, RecordSpool, MEMORY_SOURCE_LIMIT};
use super::snapshot::{compute_source_db_signature, create_history_snapshot_checked};
use crate::database::models::SyncResult;
use crate::database::repository::update_source_sync_with_reset;
use crate::error::{AppError, AppResult};

const OVERLAP_WINDOW_MILLIS: i64 = 48 * 60 * 60 * 1000; // 48 hours
const MTIME_CLOCK_TOLERANCE_MILLIS: i64 = 2_000; // 2 seconds tolerance for filesystem/clock jitter

/// Checks if the history file or its associated WAL/SHM/journal files have been modified since the last sync.
/// Returns false if definitely unchanged (can skip immediately with 0 disk I/O).
pub fn is_source_modified_since_last_sync(
    history_file_path: &Path,
    last_sync_at: Option<i64>,
) -> bool {
    is_source_modified_with_fingerprint(history_file_path, last_sync_at, None)
}

/// Checks if the history file, sidecar files (-wal/-shm/-journal), or database header fingerprint changed.
pub fn is_source_modified_with_fingerprint(
    history_file_path: &Path,
    last_sync_at: Option<i64>,
    prev_fingerprint: Option<&str>,
) -> bool {
    let last_sync_at = match last_sync_at {
        Some(t) if t > 0 => t,
        _ => return true,
    };
    let cutoff = last_sync_at.saturating_sub(MTIME_CLOCK_TOLERANCE_MILLIS);

    let get_mtime = |p: &Path| -> Option<i64> {
        std::fs::metadata(p)
            .ok()?
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_millis() as i64)
    };

    if let Some(mtime) = get_mtime(history_file_path) {
        if mtime > cutoff {
            return true;
        }
    } else {
        // Can't read metadata, assume modified to be safe
        return true;
    }

    // Check WAL, SHM and Journal files if present
    if let Some(file_name) = history_file_path.file_name().and_then(|n| n.to_str()) {
        for suffix in ["-wal", "-shm", "-journal"] {
            let sidecar = history_file_path.with_file_name(format!("{}{}", file_name, suffix));
            if sidecar.exists() {
                if let Some(sidecar_mtime) = get_mtime(&sidecar) {
                    if sidecar_mtime > cutoff {
                        return true;
                    }
                } else {
                    return true;
                }
            }
        }
    }

    // Even if mtime appears older (e.g., user restored an older backup over History),
    // verify the lightweight 100-byte header + ctime fingerprint.
    if let Some(prev) = prev_fingerprint.filter(|s| !s.is_empty()) {
        let current = compute_source_db_signature(history_file_path);
        if current.is_empty() || current != prev {
            return true;
        }
    }

    false
}

pub struct PreparedSourceData {
    pub current_signature: String,
    pub recreated: bool,
    /// Time before the source snapshot started. Changes committed while the
    /// snapshot is being imported must be eligible for the next mtime check.
    pub sync_watermark_ms: i64,
    pub records: Vec<RawVisitRecord>,
    pub(crate) spool: Option<RecordSpool>,
    pub(crate) cancelled: CancelProbe,
}

/// Stage 1: Preparation (Lock-free)
/// Uses an in-memory snapshot for small sources and an encrypted record spool
/// for large sources; preparation does not lock the archive database.
/// Performs zero database operations on the archive database, allowing read queries to proceed uninterrupted.
pub fn prepare_source_records(
    source_id: i64,
    browser: &str,
    history_file_path: &Path,
    last_visit_time: i64,
    prev_signature: Option<&str>,
    temp_dir: &Path,
) -> AppResult<PreparedSourceData> {
    prepare_source_records_checked(
        source_id,
        browser,
        history_file_path,
        last_visit_time,
        prev_signature,
        temp_dir,
        std::sync::Arc::new(|| false),
    )
}

pub(crate) fn prepare_source_records_checked(
    source_id: i64,
    browser: &str,
    history_file_path: &Path,
    last_visit_time: i64,
    prev_signature: Option<&str>,
    temp_dir: &Path,
    cancelled: CancelProbe,
) -> AppResult<PreparedSourceData> {
    let sync_watermark_ms = Utc::now().timestamp_millis();
    prepare_with_policy(
        source_id,
        browser,
        history_file_path,
        0,
        last_visit_time,
        prev_signature,
        temp_dir,
        (cancelled, MEMORY_SOURCE_LIMIT),
        sync_watermark_ms,
    )
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
pub(crate) fn prepare_source_records_with_cursor(
    source_id: i64,
    browser: &str,
    history_file_path: &Path,
    prev_last_visit_id: i64,
    last_visit_time: i64,
    prev_signature: Option<&str>,
    temp_dir: &Path,
    cancelled: CancelProbe,
) -> AppResult<PreparedSourceData> {
    prepare_source_records_with_cursor_at(
        source_id,
        browser,
        history_file_path,
        prev_last_visit_id,
        last_visit_time,
        prev_signature,
        temp_dir,
        cancelled,
        Utc::now().timestamp_millis(),
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare_source_records_with_cursor_at(
    source_id: i64,
    browser: &str,
    history_file_path: &Path,
    prev_last_visit_id: i64,
    last_visit_time: i64,
    prev_signature: Option<&str>,
    temp_dir: &Path,
    cancelled: CancelProbe,
    sync_watermark_ms: i64,
) -> AppResult<PreparedSourceData> {
    prepare_with_policy(
        source_id,
        browser,
        history_file_path,
        prev_last_visit_id,
        last_visit_time,
        prev_signature,
        temp_dir,
        (cancelled, MEMORY_SOURCE_LIMIT),
        sync_watermark_ms,
    )
}

#[allow(clippy::too_many_arguments)]
fn prepare_with_policy(
    source_id: i64,
    _browser: &str,
    history_file_path: &Path,
    prev_last_visit_id: i64,
    last_visit_time: i64,
    prev_signature: Option<&str>,
    temp_dir: &Path,
    policy: (CancelProbe, u64),
    sync_watermark_ms: i64,
) -> AppResult<PreparedSourceData> {
    let (cancelled, threshold) = policy;
    // 2. Compute signature to detect if browser recreated the History database
    let current_signature = compute_source_db_signature(history_file_path);
    let signature_changed = match prev_signature {
        Some(prev) => {
            !prev.is_empty() && !current_signature.is_empty() && prev != current_signature
        }
        None => false,
    };

    // 3. Determine since_unix_ms with 48h overlap window
    let mut recreated = signature_changed;
    let mut since_unix_ms = if signature_changed {
        info!(
            "Source #{}: Browser History file signature changed (possible recreation/clear). Switching to safe rescan.",
            source_id
        );
        0
    } else if last_visit_time > OVERLAP_WINDOW_MILLIS {
        last_visit_time - OVERLAP_WINDOW_MILLIS
    } else {
        0
    };

    // 4. Size-check and create atomic memory snapshot or encrypted spool without failing on open-browser locks
    if super::record_spool::needs_disk_spool(history_file_path, threshold, cancelled.clone())? {
        let spool = RecordSpool::prepare_auto_with_cursor_check(
            history_file_path,
            temp_dir,
            since_unix_ms,
            prev_last_visit_id,
            cancelled.clone(),
        )?;
        recreated = recreated || spool.cursor_regressed();
        return Ok(PreparedSourceData {
            current_signature,
            recreated,
            sync_watermark_ms,
            records: Vec::new(),
            spool: Some(spool),
            cancelled,
        });
    }
    if cancelled() {
        return Err(AppError::VaultLocked("保险库会话已结束".into()));
    }
    let mut snapshot_guard =
        create_history_snapshot_checked(history_file_path, temp_dir, cancelled.as_ref())?;
    let is_firefox = super::snapshot::detect_history_schema_from_conn(snapshot_guard.connection())?;
    snapshot_guard
        .connection()
        .pragma_update(None, "automatic_index", false)?;
    let probe = cancelled.clone();
    snapshot_guard
        .connection_mut()
        .progress_handler(10_000, Some(move || probe()))?;

    // Guard against NTFS tunneling (recreated file keeping old ctime):
    // if max source visit id regressed below previously committed cursor, force rescan & cursor reset.
    if !recreated && prev_last_visit_id > 0 {
        let max_id_sql = if is_firefox {
            "SELECT COALESCE(MAX(id), 0) FROM moz_historyvisits"
        } else {
            "SELECT COALESCE(MAX(id), 0) FROM visits"
        };
        if let Ok(snapshot_max_id) = snapshot_guard
            .connection()
            .query_row(max_id_sql, [], |r| r.get::<_, i64>(0))
        {
            if snapshot_max_id < prev_last_visit_id {
                info!(
                    source_id,
                    snapshot_max_id,
                    prev_last_visit_id,
                    "Source visit ID regressed; switching to safe rescan and cursor reset"
                );
                recreated = true;
                since_unix_ms = 0;
            }
        }
    }

    let parsed = if is_firefox {
        crate::browser::firefox::read_firefox_history_snapshot(
            snapshot_guard.connection(),
            since_unix_ms,
        )
    } else {
        read_chromium_history_snapshot(snapshot_guard.connection(), since_unix_ms)
    };
    let records = match parsed {
        Ok(records) => records,
        Err(AppError::MemoryPreparationLimit) => {
            drop(snapshot_guard);
            let spool = RecordSpool::prepare(
                history_file_path,
                temp_dir,
                is_firefox,
                since_unix_ms,
                cancelled.clone(),
            )?;
            return Ok(PreparedSourceData {
                current_signature,
                recreated,
                sync_watermark_ms,
                records: Vec::new(),
                spool: Some(spool),
                cancelled,
            });
        }
        Err(error) => return Err(error),
    };

    Ok(PreparedSourceData {
        current_signature,
        recreated,
        sync_watermark_ms,
        records,
        spool: None,
        cancelled,
    })
}

/// Stage 2: Commit under a single archive transaction. Streaming avoids a
/// whole-record Vec for large sources; the transaction can still take time.
pub fn commit_source_records(
    archive_conn: &mut Connection,
    source_id: i64,
    browser: &str,
    profile: &str,
    last_visit_time: i64,
    data: PreparedSourceData,
    start_time: Instant,
) -> AppResult<SyncResult> {
    commit_source_records_with_meta(
        archive_conn,
        source_id,
        browser,
        profile,
        last_visit_time,
        data,
        start_time,
        "manual",
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn commit_source_records_with_meta(
    archive_conn: &mut Connection,
    source_id: i64,
    browser: &str,
    profile: &str,
    last_visit_time: i64,
    data: PreparedSourceData,
    start_time: Instant,
    trigger_type: &str,
    run_id: Option<i64>,
) -> AppResult<SyncResult> {
    let now = Utc::now().timestamp_millis();
    let mut read_count = 0u32;

    let mut inserted_count: u32 = 0;
    let mut duplicate_count: u32 = 0;
    let failed_count: u32 = 0;
    let mut max_visit_time = last_visit_time;
    let mut max_visit_id: i64 = 0;

    {
        let tx = archive_conn.transaction()?;

        let mut select_url_id_stmt = tx.prepare_cached("SELECT id FROM urls WHERE url = ?1")?;

        let mut insert_url_stmt = tx.prepare_cached(
            r#"
            INSERT INTO urls (url, title, domain, normalized_url)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(url) DO UPDATE SET
                title = excluded.title
            WHERE length(excluded.title) > 0 AND COALESCE(urls.title, '') != excluded.title
            RETURNING id;
            "#,
        )?;

        let mut insert_visit_stmt = tx.prepare_cached(
            r#"
            INSERT OR IGNORE INTO visits (
                source_id, url_id, source_visit_id, visit_time,
                transition, from_visit, visit_duration, event_hash, imported_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
        )?;
        let mut is_excluded_stmt = tx.prepare_cached(
            "SELECT EXISTS(SELECT 1 FROM visit_exclusions WHERE event_hash = ?1)",
        )?;

        let mut consume = |r: &RawVisitRecord| -> AppResult<()> {
            if (data.cancelled)() {
                return Err(AppError::VaultLocked(
                    "保险库会话已结束，导入事务已中止".into(),
                ));
            }
            read_count = read_count
                .checked_add(1)
                .ok_or_else(|| AppError::Security("导入记录数量溢出".into()))?;
            let trimmed_url = r.url.trim();
            if trimmed_url.is_empty()
                || trimmed_url
                    .as_bytes()
                    .get(..11)
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"javascript:"))
                || trimmed_url
                    .as_bytes()
                    .get(..5)
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"data:"))
                || trimmed_url.eq_ignore_ascii_case("about:blank")
            {
                // Clean and ignore invalid / pseudo protocol URLs
                return Ok(());
            }

            let cleaned_title = r.title.trim();
            let norm = normalize_url(trimmed_url);

            let event_hash = calculate_event_hash(
                source_id,
                r.source_visit_id,
                r.visit_time_unix_ms,
                &norm.normalized_url,
            );
            if r.source_visit_id > max_visit_id {
                max_visit_id = r.source_visit_id;
            }
            if is_excluded_stmt.query_row(params![&event_hash], |row| row.get::<_, bool>(0))? {
                duplicate_count += 1;
                return Ok(());
            }
            let url_id_res: Result<i64, rusqlite::Error> = (|| {
                if let Some(id) = insert_url_stmt
                    .query_row(
                        params![
                            trimmed_url,
                            cleaned_title,
                            &norm.domain,
                            &norm.normalized_url
                        ],
                        |row| row.get(0),
                    )
                    .optional()?
                {
                    Ok(id)
                } else {
                    select_url_id_stmt.query_row(params![trimmed_url], |row| row.get(0))
                }
            })();

            let url_id = url_id_res.map_err(|_| {
                error!(source_id, "Failed to insert or resolve a history URL");
                AppError::System("历史 URL 写入失败，已回滚导入，请检查数据库后重试".into())
            })?;

            let changed = insert_visit_stmt.execute(params![
                source_id,
                url_id,
                r.source_visit_id,
                r.visit_time_unix_ms,
                r.transition,
                r.from_visit,
                r.visit_duration,
                &event_hash,
                now,
            ])?;

            if r.source_visit_id > max_visit_id {
                max_visit_id = r.source_visit_id;
            }
            if changed > 0 {
                inserted_count += 1;
                if r.visit_time_unix_ms > max_visit_time {
                    max_visit_time = r.visit_time_unix_ms;
                }
            } else {
                duplicate_count += 1;
            }
            Ok(())
        };
        if let Some(spool) = &data.spool {
            spool.for_each(|record| consume(&record))?;
            if read_count != spool.len() {
                return Err(AppError::Security(
                    "加密暂存记录数量变化，已回滚导入".into(),
                ));
            }
        } else {
            for record in &data.records {
                consume(record)?;
            }
        }

        drop(select_url_id_stmt);
        drop(insert_url_stmt);
        drop(insert_visit_stmt);
        drop(is_excluded_stmt);

        // Record job in database
        tx.execute(
            r#"
            INSERT INTO import_jobs (
                source_id, started_at, finished_at, read_count,
                inserted_count, duplicate_count, failed_count, status,
                trigger_type, run_id
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'success', ?8, ?9)
            "#,
            params![
                source_id,
                now,
                Utc::now().timestamp_millis(),
                read_count,
                inserted_count,
                duplicate_count,
                failed_count,
                trigger_type,
                run_id,
            ],
        )?;

        update_source_sync_with_reset(
            &tx,
            source_id,
            max_visit_id,
            max_visit_time,
            data.sync_watermark_ms,
            Some(&data.current_signature),
            data.recreated,
        )?;
        if (data.cancelled)() {
            return Err(AppError::VaultLocked(
                "保险库会话已结束，导入事务已回滚".into(),
            ));
        }
        tx.commit()?;
    }

    if inserted_count >= 500 {
        let _ = archive_conn.execute_batch("PRAGMA optimize;");
    }

    let duration_ms = start_time.elapsed().as_millis() as u64;

    info!(
        "Sync completed for source #{}: read={}, inserted={}, duplicate={}, failed={}, took {}ms",
        source_id, read_count, inserted_count, duplicate_count, failed_count, duration_ms
    );

    Ok(SyncResult {
        source_id,
        browser: browser.to_string(),
        profile: profile.to_string(),
        read_count,
        inserted_count,
        duplicate_count,
        failed_count,
        duration_ms,
        status: crate::database::models::SourceSyncStatus::Success,
        error: None,
    })
}

pub fn sync_source_history(
    archive_conn: &mut Connection,
    source_id: i64,
    browser: &str,
    profile: &str,
    history_file_path: &Path,
    last_visit_time: i64,
    temp_dir: &Path,
) -> AppResult<SyncResult> {
    sync_source_history_checked(
        archive_conn,
        source_id,
        browser,
        profile,
        history_file_path,
        last_visit_time,
        (temp_dir, std::sync::Arc::new(|| false)),
    )
}

pub(crate) fn sync_source_history_checked(
    archive_conn: &mut Connection,
    source_id: i64,
    browser: &str,
    profile: &str,
    history_file_path: &Path,
    last_visit_time: i64,
    preparation: (&Path, CancelProbe),
) -> AppResult<SyncResult> {
    let (temp_dir, cancelled) = preparation;
    let start_time = Instant::now();
    info!(
        "Starting sync for source #{}: {} [{}] at {:?}",
        source_id, browser, profile, history_file_path
    );

    let prev_signature: Option<String> = archive_conn
        .query_row(
            "SELECT db_fingerprint FROM sources WHERE id = ?1",
            params![source_id],
            |r| r.get(0),
        )
        .ok()
        .flatten();

    let prepared = prepare_source_records_checked(
        source_id,
        browser,
        history_file_path,
        last_visit_time,
        prev_signature.as_deref(),
        temp_dir,
        cancelled,
    )?;

    commit_source_records(
        archive_conn,
        source_id,
        browser,
        profile,
        last_visit_time,
        prepared,
        start_time,
    )
}

pub(crate) fn sync_registered_source(
    db: &crate::database::DbState,
    operation: &crate::security::session::VaultOperation,
    source: &crate::database::models::Source,
    path: &Path,
) -> AppResult<SyncResult> {
    sync_registered_source_with_meta(db, operation, source, path, "manual", None)
}

pub(crate) fn sync_registered_source_with_meta(
    db: &crate::database::DbState,
    operation: &crate::security::session::VaultOperation,
    source: &crate::database::models::Source,
    path: &Path,
    trigger_type: &str,
    run_id: Option<i64>,
) -> AppResult<SyncResult> {
    operation.check()?;
    let started = Instant::now();
    let sync_watermark_ms = Utc::now().timestamp_millis();
    let prepared = prepare_source_records_with_cursor_at(
        source.id,
        &source.browser,
        path,
        source.last_visit_id,
        source.last_visit_time,
        source.db_fingerprint.as_deref(),
        &db.temp_dir,
        operation.cancellation_probe(),
        sync_watermark_ms,
    )?;
    operation.with_conn_mut(|conn| {
        commit_source_records_with_meta(
            conn,
            source.id,
            &source.browser,
            &source.profile,
            source.last_visit_time,
            prepared,
            started,
            trigger_type,
            run_id,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    #[test]
    fn parser_uses_schema_even_when_filename_and_browser_are_misleading() {
        for firefox in [false, true] {
            let root =
                std::env::temp_dir().join(format!("browsory_schema_{}", uuid::Uuid::new_v4()));
            let (source, writer) =
                super::super::record_spool::tests::source_fixture(&root, firefox);
            drop(writer);
            let renamed = root.join(if firefox {
                "firefox-backup.db"
            } else {
                "places.sqlite.backup"
            });
            std::fs::rename(&source, &renamed).unwrap();
            let prepared =
                prepare_source_records(1, "custom", &renamed, 0, None, &root.join("temp")).unwrap();
            assert_eq!(prepared.records.len(), 2);
            drop(prepared);
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn url_write_failure_rolls_back_newer_rows_and_cursor_then_retries() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::database::migrations::run_migrations(&mut conn).unwrap();
        let source = crate::database::repository::get_or_create_source(
            &conn, "custom", "test", "sqlite", "fixture",
        )
        .unwrap();
        let prepare = || PreparedSourceData {
            current_signature: "fixture".into(),
            recreated: false,
            sync_watermark_ms: Utc::now().timestamp_millis(),
            spool: None,
            cancelled: Arc::new(|| false),
            records: [(1, 1_000_000, "old"), (2, 300_000_000, "new")]
                .into_iter()
                .map(|(id, time, name)| RawVisitRecord {
                    source_visit_id: id,
                    visit_time_unix_ms: time,
                    from_visit: 0,
                    transition: 1,
                    visit_duration: 0,
                    url: format!("https://example.org/{name}"),
                    title: name.into(),
                })
                .collect(),
        };
        conn.execute_batch("CREATE TRIGGER reject_new BEFORE INSERT ON urls WHEN NEW.title='new' BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
        assert!(commit_source_records(
            &mut conn,
            source.id,
            "custom",
            "test",
            0,
            prepare(),
            Instant::now()
        )
        .is_err());
        let count: i64 = conn
            .query_row("SELECT count(*) FROM visits", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
        let cursor: i64 = conn
            .query_row(
                "SELECT last_visit_time FROM sources WHERE id=?",
                [source.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cursor, 0);
        conn.execute_batch("DROP TRIGGER reject_new").unwrap();
        assert_eq!(
            commit_source_records(
                &mut conn,
                source.id,
                "custom",
                "test",
                0,
                prepare(),
                Instant::now()
            )
            .unwrap()
            .inserted_count,
            2
        );
    }

    #[test]
    fn user_deleted_visit_is_not_reimported_and_sync_keeps_snapshot_watermark() {
        let mut archive = Connection::open_in_memory().unwrap();
        crate::database::migrations::run_migrations(&mut archive).unwrap();
        let source = crate::database::repository::get_or_create_source(
            &archive,
            "chrome",
            "Default",
            "sqlite",
            "synthetic-source",
        )
        .unwrap();
        let record = RawVisitRecord {
            source_visit_id: 41,
            visit_time_unix_ms: 1_700_000_000_000,
            from_visit: 0,
            transition: 1,
            visit_duration: 0,
            url: "https://deleted.example.test/".into(),
            title: "Deleted visit".into(),
        };
        let watermark = Utc::now().timestamp_millis() - 60_000;
        let prepared = || PreparedSourceData {
            current_signature: "synthetic-signature".into(),
            recreated: false,
            sync_watermark_ms: watermark,
            records: vec![record.clone()],
            spool: None,
            cancelled: Arc::new(|| false),
        };

        let first = commit_source_records(
            &mut archive,
            source.id,
            "chrome",
            "Default",
            0,
            prepared(),
            Instant::now(),
        )
        .unwrap();
        assert_eq!(first.inserted_count, 1);
        let visit_id = archive
            .query_row("SELECT id FROM visits", [], |row| row.get::<_, i64>(0))
            .unwrap();
        crate::database::repository::delete_visits(&archive, &[visit_id]).unwrap();

        let second = commit_source_records(
            &mut archive,
            source.id,
            "chrome",
            "Default",
            0,
            prepared(),
            Instant::now(),
        )
        .unwrap();
        assert_eq!(second.inserted_count, 0);
        assert_eq!(second.duplicate_count, 1);
        assert_eq!(
            archive
                .query_row("SELECT count(*) FROM visits", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            archive
                .query_row("SELECT count(*) FROM visit_exclusions", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        let source = crate::database::repository::get_source_by_id(&archive, source.id)
            .unwrap()
            .unwrap();
        assert_eq!(source.last_sync_at, Some(watermark));
    }

    #[test]
    fn small_source_with_expanded_titles_switches_to_encrypted_spool() {
        for firefox in [false, true] {
            let root = std::env::temp_dir().join(format!(
                "browsory_expanded_records_{}",
                uuid::Uuid::new_v4()
            ));
            let app = root.join("app");
            std::fs::create_dir_all(&app).unwrap();
            let (source, writer) =
                super::super::record_spool::tests::source_fixture(&root.join("browser"), firefox);
            writer
                .execute(
                    if firefox {
                        "UPDATE moz_places SET title=?1"
                    } else {
                        "UPDATE urls SET title=?1"
                    },
                    ["x".repeat(256 * 1024)],
                )
                .unwrap();
            writer.execute_batch(if firefox {
                "WITH RECURSIVE n(i) AS(SELECT 3 UNION ALL SELECT i+1 FROM n WHERE i<300) INSERT INTO moz_historyvisits SELECT i,1,1700000000000000+i,0,1 FROM n;"
            }else{
                "WITH RECURSIVE n(i) AS(SELECT 3 UNION ALL SELECT i+1 FROM n WHERE i<300) INSERT INTO visits SELECT i,1,13344473600000000+i,0,1,0 FROM n;"
            }).unwrap();
            assert!(!super::super::record_spool::needs_disk_spool(
                &source,
                MEMORY_SOURCE_LIMIT,
                Arc::new(|| false)
            )
            .unwrap());
            let prepared = prepare_source_records(
                1,
                if firefox { "firefox" } else { "chrome" },
                &source,
                0,
                None,
                &app.join("temp"),
            )
            .unwrap();
            assert!(prepared.records.is_empty());
            assert_eq!(prepared.spool.as_ref().unwrap().len(), 300);
            drop(prepared);
            assert!(!app.join("temp").exists());
            assert_eq!(
                std::fs::read_dir(app.join("snapshot-work"))
                    .unwrap()
                    .count(),
                0
            );
            drop(writer);
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn source_over_256_mib_routes_to_encrypted_spool_and_commits_without_record_vector() {
        let root =
            std::env::temp_dir().join(format!("browsory_large_import_{}", uuid::Uuid::new_v4()));
        let app = root.join("app");
        std::fs::create_dir_all(&app).unwrap();
        let (source, writer) =
            super::super::record_spool::tests::source_fixture(&root.join("browser"), false);
        writer.execute_batch("CREATE TABLE padding(value BLOB); INSERT INTO padding VALUES(zeroblob(272629760));").unwrap();
        let size: u64 = writer
            .query_row(
                "SELECT page_count*page_size FROM pragma_page_count(),pragma_page_size()",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(size > MEMORY_SOURCE_LIMIT);
        let vault = crate::security::vault::VaultManager::unlocked_fixture(app.clone());
        vault
            .with_conn_mut(|archive| {
                let row = crate::database::repository::get_or_create_source(
                    archive,
                    "chrome",
                    "Default",
                    "sqlite",
                    &source.to_string_lossy(),
                )?;
                let prepared =
                    prepare_source_records(row.id, "chrome", &source, 0, None, &app.join("temp"))?;
                assert!(prepared.records.is_empty());
                assert!(prepared.spool.is_some());
                let result = commit_source_records(
                    archive,
                    row.id,
                    "chrome",
                    "Default",
                    0,
                    prepared,
                    Instant::now(),
                )?;
                assert_eq!(result.read_count, 2);
                assert_eq!(result.inserted_count, 2);
                let repeated =
                    prepare_source_records(row.id, "chrome", &source, 0, None, &app.join("temp"))?;
                let result = commit_source_records(
                    archive,
                    row.id,
                    "chrome",
                    "Default",
                    0,
                    repeated,
                    Instant::now(),
                )?;
                assert_eq!(result.inserted_count, 0);
                assert_eq!(result.duplicate_count, 2);
                Ok(())
            })
            .unwrap();
        assert_eq!(
            std::fs::read_dir(app.join("snapshot-work"))
                .unwrap()
                .count(),
            0
        );
        assert!(!app.join("temp").exists());
        vault.lock().unwrap();
        drop(vault);
        drop(writer);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cancellation_during_stream_commit_and_metadata_failure_roll_back_every_row() {
        use rusqlite::functions::FunctionFlags;
        use std::sync::atomic::{AtomicBool, Ordering};
        let root =
            std::env::temp_dir().join(format!("browsory_stream_rollback_{}", uuid::Uuid::new_v4()));
        let app = root.join("app");
        std::fs::create_dir_all(&app).unwrap();
        let (source, writer) =
            super::super::record_spool::tests::source_fixture(&root.join("browser"), false);
        let vault = crate::security::vault::VaultManager::unlocked_fixture(app.clone());
        vault.with_conn_mut(|archive| {
            let row=crate::database::repository::get_or_create_source(archive,"chrome","Default","sqlite",&source.to_string_lossy())?;
            let cancelled=Arc::new(AtomicBool::new(false));let trigger=cancelled.clone();
            archive.create_scalar_function("cancel_stream",0,FunctionFlags::SQLITE_UTF8,move |_|{trigger.store(true,Ordering::Release);Ok(0i64)})?;
            archive.execute_batch("CREATE TRIGGER cancel_import AFTER INSERT ON visits BEGIN SELECT cancel_stream(); END;")?;
            let probe=Arc::new(move ||cancelled.load(Ordering::Acquire));
            let prepared=prepare_with_policy(row.id,"chrome",&source,0,0,None,&app.join("temp"),(probe,0),Utc::now().timestamp_millis())?;
            assert!(commit_source_records(archive,row.id,"chrome","Default",0,prepared,Instant::now()).is_err());
            assert_eq!(archive.query_row("SELECT count(*) FROM visits",[],|r|r.get::<_,i64>(0))?,0);
            assert_eq!(archive.query_row("SELECT count(*) FROM import_jobs",[],|r|r.get::<_,i64>(0))?,0);
            archive.execute_batch("DROP TRIGGER cancel_import; CREATE TRIGGER fail_source_update BEFORE UPDATE ON sources BEGIN SELECT RAISE(ABORT,'synthetic metadata failure'); END;")?;
            let prepared=prepare_with_policy(row.id,"chrome",&source,0,0,None,&app.join("temp"),(Arc::new(||false),0),Utc::now().timestamp_millis())?;
            assert!(commit_source_records(archive,row.id,"chrome","Default",0,prepared,Instant::now()).is_err());
            assert_eq!(archive.query_row("SELECT count(*) FROM visits",[],|r|r.get::<_,i64>(0))?,0);
            assert_eq!(archive.query_row("SELECT count(*) FROM import_jobs",[],|r|r.get::<_,i64>(0))?,0);
            assert_eq!(archive.query_row("SELECT count(*) FROM sources WHERE last_sync_at IS NOT NULL",[],|r|r.get::<_,i64>(0))?,0);
            Ok(())
        }).unwrap();
        assert_eq!(
            std::fs::read_dir(app.join("snapshot-work"))
                .unwrap()
                .count(),
            0
        );
        vault.lock().unwrap();
        drop(vault);
        drop(writer);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[derive(Clone)]
    struct LogBuffer(Arc<Mutex<Vec<u8>>>);
    impl Write for LogBuffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn chromium_and_firefox_import_use_memory_and_error_logs_omit_history() {
        let root =
            std::env::temp_dir().join(format!("browsory_memory_import_{}", uuid::Uuid::new_v4()));
        let app_dir = root.join("app");
        std::fs::create_dir_all(&app_dir).unwrap();
        let vault = crate::security::vault::VaultManager::unlocked_fixture(app_dir.clone());
        let marker = "PRIVATE-SYNC-LOG-SENTINEL";
        for browser in ["chrome", "firefox"] {
            let source = root.join(format!("{browser}.db"));
            let conn = Connection::open(&source).unwrap();
            conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get::<_, String>(0))
                .unwrap();
            if browser == "chrome" {
                conn.execute_batch("CREATE TABLE urls(id INTEGER PRIMARY KEY,url TEXT,title TEXT); CREATE TABLE visits(id INTEGER PRIMARY KEY,url INTEGER,visit_time INTEGER,from_visit INTEGER,transition INTEGER,visit_duration INTEGER);").unwrap();
                conn.execute(
                    "INSERT INTO urls VALUES(1,?1,?2)",
                    params![format!("https://example.test/{marker}"), marker],
                )
                .unwrap();
                conn.execute(
                    "INSERT INTO visits VALUES(1,1,?1,0,1,0)",
                    [super::super::parser::unix_millis_to_webkit_micros(
                        1700000000000,
                    )],
                )
                .unwrap();
            } else {
                conn.execute_batch("CREATE TABLE moz_places(id INTEGER PRIMARY KEY,url TEXT,title TEXT); CREATE TABLE moz_historyvisits(id INTEGER PRIMARY KEY,place_id INTEGER,visit_date INTEGER,from_visit INTEGER,visit_type INTEGER);").unwrap();
                conn.execute(
                    "INSERT INTO moz_places VALUES(1,?1,?2)",
                    params![format!("https://example.test/{marker}"), marker],
                )
                .unwrap();
                conn.execute(
                    "INSERT INTO moz_historyvisits VALUES(1,1,1700000000000000,0,1)",
                    [],
                )
                .unwrap();
            }
            vault
                .with_conn_mut(|archive| {
                    let row = crate::database::repository::get_or_create_source(
                        archive,
                        browser,
                        "Default",
                        "sqlite",
                        &source.to_string_lossy(),
                    )?;
                    let prepared = prepare_source_records(
                        row.id,
                        browser,
                        &source,
                        0,
                        None,
                        &app_dir.join("temp"),
                    )?;
                    assert_eq!(prepared.records.len(), 1);
                    assert_eq!(prepared.records[0].title, marker);
                    assert_eq!(prepared.records[0].visit_time_unix_ms, 1700000000000);
                    assert!(!app_dir.join("temp").exists());
                    let imported = commit_source_records(
                        archive,
                        row.id,
                        browser,
                        "Default",
                        0,
                        prepared,
                        Instant::now(),
                    )?;
                    assert_eq!(imported.inserted_count, 1);
                    Ok(())
                })
                .unwrap();
            drop(conn);
        }
        let buffer = LogBuffer(Arc::new(Mutex::new(Vec::new())));
        let writer = buffer.clone();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            vault.with_conn_mut(|archive| {
                archive.execute_batch(&format!("CREATE TRIGGER refuse_private_url BEFORE INSERT ON urls BEGIN SELECT RAISE(ABORT, '{marker}'); END;"))?;
                let row = crate::database::repository::get_or_create_source(archive, "test", "Default", "sqlite", "synthetic")?;
                let prepared = PreparedSourceData { current_signature: "test".into(), recreated: false, sync_watermark_ms: Utc::now().timestamp_millis(), spool:None,cancelled:std::sync::Arc::new(||false), records: vec![RawVisitRecord {
                    source_visit_id: 2, visit_time_unix_ms: 1700000000001, from_visit: 0, transition: 1, visit_duration: 0,
                    url: format!("https://example.test/{marker}"), title: marker.into(),
                }] };
                let result = commit_source_records(archive, row.id, "test", "Default", 0, prepared, Instant::now());
                assert!(result.is_err()); Ok(())
            }).unwrap();
        });
        let output = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
        assert!(output.contains("Failed to insert or resolve"));
        assert!(!output.contains(marker));
        assert!(!output.contains("https://example.test"));
        vault.lock().unwrap();
        let db_bytes = std::fs::read(app_dir.join("archive.db")).unwrap();
        assert!(!db_bytes
            .windows(marker.len())
            .any(|window| window == marker.as_bytes()));
        drop(vault);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ntfs_tunneling_id_regression_triggers_rescan_and_resets_cursor() {
        let root =
            std::env::temp_dir().join(format!("browsory_id_regress_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let source_path = root.join("History");
        {
            let conn = Connection::open(&source_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE urls(id INTEGER PRIMARY KEY, url TEXT, title TEXT);
                 CREATE TABLE visits(id INTEGER PRIMARY KEY, url INTEGER, visit_time INTEGER, from_visit INTEGER, transition INTEGER, visit_duration INTEGER);
                 INSERT INTO urls VALUES(1, 'https://recreated.test/page', 'Recreated');
                 INSERT INTO visits VALUES(1, 1, 13344473600000000, 0, 1, 0);",
            )
            .unwrap();
        }
        let sig = compute_source_db_signature(&source_path);
        // Simulate previous cursor at visit_id=500, last_visit_time far ahead, but same signature (NTFS tunneling)
        let prepared = prepare_source_records_with_cursor(
            1,
            "chrome",
            &source_path,
            500,
            1_800_000_000_000,
            Some(&sig),
            &root.join("temp"),
            Arc::new(|| false),
        )
        .unwrap();
        assert!(prepared.recreated);
        assert_eq!(prepared.records.len(), 1);

        let mut archive = Connection::open_in_memory().unwrap();
        crate::database::migrations::run_migrations(&mut archive).unwrap();
        let src = crate::database::repository::get_or_create_source(
            &archive,
            "chrome",
            "Default",
            "auto",
            &source_path.to_string_lossy(),
        )
        .unwrap();
        crate::database::repository::update_source_sync(
            &archive,
            src.id,
            500,
            1_800_000_000_000,
            1_800_000_000_000,
            Some(&sig),
        )
        .unwrap();
        let res = commit_source_records_with_meta(
            &mut archive,
            src.id,
            "chrome",
            "Default",
            1_800_000_000_000,
            prepared,
            Instant::now(),
            "scheduled",
            None,
        )
        .unwrap();
        assert_eq!(res.inserted_count, 1);
        let updated = crate::database::repository::get_source_by_id(&archive, src.id)
            .unwrap()
            .unwrap();
        assert_eq!(updated.last_visit_id, 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn prepare_source_records_succeeds_while_chromium_holds_exclusive_lock() {
        let root =
            std::env::temp_dir().join(format!("browsory_sync_exclusive_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let history_path = root.join("History");
        let temp_dir = root.join("app").join("temp");

        let holder = Connection::open(&history_path).unwrap();
        holder
            .query_row("PRAGMA journal_mode=PERSIST", [], |r| r.get::<_, String>(0))
            .unwrap();
        holder
            .query_row("PRAGMA locking_mode=EXCLUSIVE", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap();
        holder
            .execute_batch(
                "CREATE TABLE urls(id INTEGER PRIMARY KEY, url TEXT, title TEXT);
                 CREATE TABLE visits(id INTEGER PRIMARY KEY, url INTEGER, visit_time INTEGER, from_visit INTEGER, transition INTEGER, visit_duration INTEGER);
                 INSERT INTO urls VALUES(1, 'https://docs.rs/rusqlite', 'rusqlite');
                 INSERT INTO visits VALUES(101, 1, 13380000000000000, 0, 0, 5000000);",
            )
            .unwrap();
        holder
            .execute(
                "INSERT INTO urls VALUES(2, 'https://tauri.app/', 'Tauri')",
                [],
            )
            .unwrap();
        holder
            .execute(
                "INSERT INTO visits VALUES(102, 2, 13380000001000000, 0, 0, 2000000)",
                [],
            )
            .unwrap();

        let prepared =
            prepare_source_records(0, "chrome", &history_path, 0, None, &temp_dir).unwrap();
        assert_eq!(prepared.records.len(), 2);
        assert!(prepared
            .records
            .iter()
            .any(|r| r.url == "https://tauri.app/"));

        drop(holder);
        let _ = std::fs::remove_dir_all(root);
    }
}
