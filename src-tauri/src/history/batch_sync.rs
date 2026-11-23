//! One result for every registered source, shared by manual and background sync.
use std::path::Path;
use std::time::Instant;

use crate::database::models::{Source, SourceSyncStatus, SyncResult};
use crate::database::{repository, DbState};
use crate::error::AppResult;
use crate::history::record_spool::CancelProbe;
use crate::security::session::VaultOperation;
use rusqlite::Connection;
use std::sync::Arc;

enum SyncDbTarget<'a> {
    Operation {
        db: &'a DbState,
        operation: &'a VaultOperation,
    },
    Direct {
        conn: &'a mut Connection,
        temp_dir: &'a Path,
        cancelled: CancelProbe,
    },
}

impl<'a> SyncDbTarget<'a> {
    fn check(&self) -> AppResult<()> {
        match self {
            Self::Operation { operation, .. } => operation.check(),
            Self::Direct { cancelled, .. } => {
                if cancelled() {
                    Err(crate::error::AppError::VaultLocked(
                        "后台同步会话已中止".into(),
                    ))
                } else {
                    Ok(())
                }
            }
        }
    }

    fn temp_dir(&self) -> &Path {
        match self {
            Self::Operation { db, .. } => &db.temp_dir,
            Self::Direct { temp_dir, .. } => temp_dir,
        }
    }

    fn cancellation_probe(&self) -> CancelProbe {
        match self {
            Self::Operation { operation, .. } => operation.cancellation_probe(),
            Self::Direct { cancelled, .. } => cancelled.clone(),
        }
    }

    fn with_conn<F, R>(&self, f: F) -> AppResult<R>
    where
        F: FnOnce(&Connection) -> AppResult<R>,
    {
        match self {
            Self::Operation { operation, .. } => operation.with_conn(f),
            Self::Direct { conn, .. } => f(conn),
        }
    }

    fn with_read_conn<F, R>(&self, f: F) -> AppResult<R>
    where
        F: FnOnce(&Connection) -> AppResult<R>,
    {
        match self {
            Self::Operation { operation, .. } => operation.with_read_conn(f),
            Self::Direct { conn, .. } => f(conn),
        }
    }

    fn with_conn_mut<F, R>(&mut self, f: F) -> AppResult<R>
    where
        F: FnOnce(&mut Connection) -> AppResult<R>,
    {
        match self {
            Self::Operation { operation, .. } => operation.with_conn_mut(f),
            Self::Direct { conn, .. } => f(conn),
        }
    }
}

fn empty(source: &Source, status: SourceSyncStatus, error: Option<String>) -> SyncResult {
    SyncResult {
        source_id: source.id,
        browser: source.browser.clone(),
        profile: source.profile.clone(),
        read_count: 0,
        inserted_count: 0,
        duplicate_count: 0,
        failed_count: 0,
        duration_ms: 0,
        status,
        error,
    }
}

fn record_failure(
    target: &SyncDbTarget<'_>,
    source: &Source,
    message: &str,
    trigger_type: &str,
    run_id: Option<i64>,
) {
    let now = chrono::Utc::now().timestamp_millis();
    let result = target.with_conn(|conn| {
        // Repeated watcher events should not create an unlimited stream of the same failure.
        conn.execute(
            "INSERT INTO import_jobs(source_id,started_at,finished_at,failed_count,status,error,trigger_type,run_id) SELECT ?1,?2,?2,0,'failed',?3,?5,?6 WHERE NOT EXISTS(SELECT 1 FROM import_jobs WHERE source_id=?1 AND status='failed' AND error=?3 AND started_at>?4)",
            rusqlite::params![source.id, now, message, now - 1_800_000, trigger_type, run_id],
        )?;
        Ok(())
    });
    if result.is_err() {
        tracing::warn!(
            source_id = source.id,
            "Could not persist source failure; returned result remains failed"
        );
    }
}

pub(crate) fn is_transient_sync_error(err: &crate::error::AppError) -> bool {
    if super::snapshot::is_app_error_locked(err) {
        return true;
    }
    let msg = err.to_string();
    msg.contains("正在写入事务中")
        || msg.contains("检测到并发写入")
        || msg.contains("内存快照超时")
        || msg.contains("发生变更，请稍后重试")
        || msg.contains("database is locked")
        || msg.contains("database is busy")
}

pub fn sync_all_registered(db: &DbState, operation: &VaultOperation) -> AppResult<Vec<SyncResult>> {
    sync_all_registered_with_options(db, operation, "manual", None, 1)
}

pub fn sync_all_registered_with_options(
    db: &DbState,
    operation: &VaultOperation,
    trigger_type: &str,
    run_id: Option<i64>,
    max_transient_retries: u32,
) -> AppResult<Vec<SyncResult>> {
    sync_all_internal(
        SyncDbTarget::Operation { db, operation },
        trigger_type,
        run_id,
        max_transient_retries,
    )
}

pub fn sync_all_with_conn(
    conn: &mut Connection,
    temp_dir: &Path,
    trigger_type: &str,
    run_id: Option<i64>,
    max_transient_retries: u32,
) -> AppResult<Vec<SyncResult>> {
    sync_all_internal(
        SyncDbTarget::Direct {
            conn,
            temp_dir,
            cancelled: Arc::new(|| false),
        },
        trigger_type,
        run_id,
        max_transient_retries,
    )
}

fn sync_all_internal(
    mut target: SyncDbTarget<'_>,
    trigger_type: &str,
    run_id: Option<i64>,
    max_transient_retries: u32,
) -> AppResult<Vec<SyncResult>> {
    let sources = target.with_read_conn(repository::list_sources)?;
    let mut results = Vec::with_capacity(sources.len());
    const RETRY_BACKOFF_SECS: [u64; 3] = [1, 3, 8];

    for source in sources {
        target.check()?;
        // Takeout JSON and previously imported manual snapshots whose original
        // file has since been removed must not fail background sync batches.
        if !source.enabled || source.source_type == "takeout" {
            results.push(empty(&source, SourceSyncStatus::Skipped, None));
            continue;
        }
        let path = Path::new(&source.history_path);
        if source.source_type == "manual" && source.last_sync_at.is_some() && !path.is_file() {
            results.push(empty(&source, SourceSyncStatus::Skipped, None));
            continue;
        }
        let started = Instant::now();
        // The mtime gate and the snapshot share one conservative watermark.
        // A write after this instant must be checked again on the next run.
        let sync_watermark_ms = chrono::Utc::now().timestamp_millis();
        let result = if !path.is_file() {
            Err(crate::error::AppError::Browser(
                "历史文件不存在或不再是普通文件，请重新扫描或选择来源".into(),
            ))
        } else if !super::sync::is_source_modified_with_fingerprint(
            path,
            source.last_sync_at,
            source.db_fingerprint.as_deref(),
        ) {
            results.push(empty(&source, SourceSyncStatus::Skipped, None));
            continue;
        } else {
            let mut attempt = 0u32;
            loop {
                target.check()?;
                let attempt_res = (|| {
                    let prepared = super::sync::prepare_source_records_with_cursor_at(
                        source.id,
                        &source.browser,
                        path,
                        source.last_visit_id,
                        source.last_visit_time,
                        source.db_fingerprint.as_deref(),
                        target.temp_dir(),
                        target.cancellation_probe(),
                        sync_watermark_ms,
                    )?;
                    target.with_conn_mut(|conn| {
                        super::sync::commit_source_records_with_meta(
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
                })();
                match attempt_res {
                    Ok(ok) => break Ok(ok),
                    Err(err)
                        if attempt < max_transient_retries && is_transient_sync_error(&err) =>
                    {
                        let wait_secs = RETRY_BACKOFF_SECS
                            .get(attempt as usize)
                            .copied()
                            .unwrap_or(8);
                        tracing::info!(
                            source_id = source.id,
                            attempt = attempt + 1,
                            wait_secs,
                            "Transient source lock during sync; backing off before retry"
                        );
                        let steps = wait_secs.saturating_mul(10);
                        for _ in 0..steps {
                            target.check()?;
                            std::thread::sleep(std::time::Duration::from_millis(100));
                        }
                        target.check()?;
                        attempt += 1;
                    }
                    Err(err) => break Err(err),
                }
            }
        };
        match result {
            Ok(result) => results.push(result),
            Err(error) => {
                target.check()?; // Revocation is a batch cancellation, never a failed source.
                let message = crate::diagnostics::sanitize_log_text(&error.to_string())
                    .chars()
                    .take(600)
                    .collect::<String>();
                record_failure(&target, &source, &message, trigger_type, run_id);
                let mut failed = empty(&source, SourceSyncStatus::Failed, Some(message));
                failed.duration_ms = started.elapsed().as_millis() as u64;
                results.push(failed);
            }
        }
    }
    target.check()?;
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{params, Connection};
    use std::sync::Arc;
    #[test]
    fn batch_reports_missing_disabled_commit_failure_and_success_without_losing_rows() {
        let root =
            std::env::temp_dir().join(format!("browsory_batch_sync_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let history = root.join("History");
        let source = Connection::open(&history).unwrap();
        source.execute_batch("CREATE TABLE urls(id INTEGER PRIMARY KEY,url TEXT,title TEXT);CREATE TABLE visits(id INTEGER PRIMARY KEY,url INTEGER,visit_time INTEGER,from_visit INTEGER,transition INTEGER,visit_duration INTEGER);INSERT INTO urls VALUES(1,'https://sync-fixture.test/','Synthetic sync fixture');").unwrap();
        let time =
            super::super::unix_millis_to_webkit_micros(chrono::Utc::now().timestamp_millis());
        source
            .execute("INSERT INTO visits VALUES(1,1,?1,0,1,10)", [time])
            .unwrap();
        drop(source);
        let data = root.join("data");
        std::fs::create_dir(&data).unwrap();
        let db = DbState {
            _directory_lock: None,
            vault: Arc::new(crate::security::vault::VaultManager::unlocked_fixture(
                data.clone(),
            )),
            app_dir: data.clone(),
            temp_dir: data.join("temp"),
            logs_dir: data.join("logs"),
        };
        let ids=db.with_conn(|conn|{
            let good=repository::get_or_create_source(conn,"chrome","Good","sqlite",&history.to_string_lossy())?.id;
            let bad=repository::get_or_create_source(conn,"chrome","CommitFailure","sqlite",&history.to_string_lossy())?.id;
            let missing=repository::get_or_create_source(conn,"chrome","Missing","sqlite",&root.join("missing").to_string_lossy())?.id;
            let disabled=repository::get_or_create_source(conn,"chrome","Disabled","sqlite",&root.join("disabled").to_string_lossy())?.id;
            conn.execute("UPDATE sources SET enabled=0 WHERE id=?1",[disabled])?;
            conn.execute_batch(&format!("CREATE TRIGGER fixture_commit_failure BEFORE INSERT ON visits WHEN NEW.source_id={bad} BEGIN SELECT RAISE(ABORT,'synthetic commit failure'); END;"))?;
            Ok((good,bad,missing,disabled))
        }).unwrap();
        let operation = db.begin_operation().unwrap();
        let results = sync_all_registered(&db, &operation).unwrap();
        assert_eq!(results.len(), 4);
        let result = |id| results.iter().find(|r| r.source_id == id).unwrap();
        assert_eq!(result(ids.0).status, SourceSyncStatus::Success);
        assert_eq!(result(ids.0).inserted_count, 1);
        assert_eq!(result(ids.1).status, SourceSyncStatus::Failed);
        assert_eq!(result(ids.2).status, SourceSyncStatus::Failed);
        assert_eq!(result(ids.3).status, SourceSyncStatus::Skipped);
        assert!(result(ids.2).error.as_ref().unwrap().contains("不存在"));
        operation
            .with_conn(|conn| {
                assert_eq!(
                    conn.query_row("SELECT COUNT(*) FROM visits", [], |r| r.get::<_, i64>(0))?,
                    1
                );
                assert_eq!(
                    conn.query_row(
                        "SELECT COUNT(*) FROM import_jobs WHERE status='failed'",
                        [],
                        |r| r.get::<_, i64>(0)
                    )?,
                    2
                );
                conn.execute("UPDATE sources SET enabled=0 WHERE id=?1", params![ids.0])?;
                Ok(())
            })
            .unwrap();
        let repeat = sync_all_registered(&db, &operation).unwrap();
        assert!(!repeat.iter().any(|r| r.status == SourceSyncStatus::Success));
        assert_eq!(
            repeat
                .iter()
                .filter(|r| r.status == SourceSyncStatus::Failed)
                .count(),
            2
        );
        operation
            .with_conn(|conn| {
                assert_eq!(
                    conn.query_row(
                        "SELECT COUNT(*) FROM import_jobs WHERE status='failed'",
                        [],
                        |r| r.get::<_, i64>(0)
                    )?,
                    2
                );
                Ok(())
            })
            .unwrap();
        drop(operation);
        db.vault.lock().unwrap();
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn deleted_after_import_manual_source_skips_without_error_and_retains_visits() {
        let root =
            std::env::temp_dir().join(format!("browsory_manual_deleted_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let history = root.join("ManualHistory");
        let source = Connection::open(&history).unwrap();
        source.execute_batch("CREATE TABLE urls(id INTEGER PRIMARY KEY,url TEXT,title TEXT);CREATE TABLE visits(id INTEGER PRIMARY KEY,url INTEGER,visit_time INTEGER,from_visit INTEGER,transition INTEGER,visit_duration INTEGER);INSERT INTO urls VALUES(1,'https://manual-import.test/','Manual');").unwrap();
        let time =
            super::super::unix_millis_to_webkit_micros(chrono::Utc::now().timestamp_millis());
        source
            .execute("INSERT INTO visits VALUES(1,1,?1,0,1,10)", [time])
            .unwrap();
        drop(source);

        let data = root.join("data");
        std::fs::create_dir(&data).unwrap();
        let db = DbState {
            _directory_lock: None,
            vault: Arc::new(crate::security::vault::VaultManager::unlocked_fixture(
                data.clone(),
            )),
            app_dir: data.clone(),
            temp_dir: data.join("temp"),
            logs_dir: data.join("logs"),
        };
        let manual_id = db
            .with_conn(|conn| {
                Ok(repository::get_or_create_source(
                    conn,
                    "custom",
                    "ManualHistory",
                    "manual",
                    &history.to_string_lossy(),
                )?
                .id)
            })
            .unwrap();

        let operation = db.begin_operation().unwrap();
        let first = sync_all_registered(&db, &operation).unwrap();
        assert_eq!(first[0].status, SourceSyncStatus::Success);
        assert_eq!(first[0].inserted_count, 1);

        // User deletes the imported source file; subsequent sync_all skips it and keeps visits intact.
        std::fs::remove_file(&history).unwrap();
        let second = sync_all_registered(&db, &operation).unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].source_id, manual_id);
        assert_eq!(second[0].status, SourceSyncStatus::Skipped);
        operation
            .with_conn(|conn| {
                assert_eq!(
                    conn.query_row("SELECT COUNT(*) FROM visits", [], |r| r.get::<_, i64>(0))?,
                    1
                );
                Ok(())
            })
            .unwrap();

        drop(operation);
        db.vault.lock().unwrap();
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
}
