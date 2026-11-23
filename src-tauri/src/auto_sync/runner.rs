use chrono::Utc;
use rusqlite::Connection;
use std::path::Path;
use std::time::Instant;
use tracing::{info, warn};
use uuid::Uuid;

use super::scheduler;
use super::state_file;
use crate::database::models::{
    AutoSyncRunItem, AutoSyncSourceSummary, SchedulerHealthStatus, SourceSyncStatus, SyncResult,
    SyncTriggerType,
};
use crate::database::{repository, DbState};
use crate::error::AppResult;
use crate::history::batch_sync;
use crate::history::coordinator::global_sync_coordinator;
use crate::security::session::VaultOperation;

pub const MILLIS_PER_DAY: i64 = 86_400_000;

pub fn compute_next_run_at(anchor_ms: i64, interval_days: u32) -> i64 {
    anchor_ms.saturating_add((interval_days.max(1) as i64).saturating_mul(MILLIS_PER_DAY))
}

pub enum HeadlessRunOutcome {
    Completed {
        run: AutoSyncRunItem,
        all_transient_failed: bool,
    },
    SkippedDisabled,
    SkippedNotDue,
    SkippedNoSources,
}

struct AggregatedRun {
    status: &'static str,
    sources_total: u32,
    sources_succeeded: u32,
    sources_skipped: u32,
    sources_failed: u32,
    total_read: u32,
    total_inserted: u32,
    total_duplicate: u32,
    error_summary: Option<String>,
    details: Vec<AutoSyncSourceSummary>,
    all_transient_failed: bool,
}

fn aggregate_results(results: &[SyncResult]) -> AggregatedRun {
    let mut sources_succeeded = 0u32;
    let mut sources_skipped = 0u32;
    let mut sources_failed = 0u32;
    let mut total_read = 0u32;
    let mut total_inserted = 0u32;
    let mut total_duplicate = 0u32;
    let mut errors = Vec::new();
    let mut details = Vec::with_capacity(results.len());
    let mut transient_failures = 0u32;

    for r in results {
        match r.status {
            SourceSyncStatus::Success => sources_succeeded += 1,
            SourceSyncStatus::Skipped => sources_skipped += 1,
            SourceSyncStatus::Failed => {
                sources_failed += 1;
                if let Some(err) = &r.error {
                    let msg = crate::diagnostics::sanitize_log_text(err);
                    if msg.contains("locked")
                        || msg.contains("busy")
                        || msg.contains("并发写入")
                        || msg.contains("写入事务中")
                        || msg.contains("内存快照超时")
                        || msg.contains("发生变更，请稍后重试")
                    {
                        transient_failures += 1;
                    }
                    errors.push(format!("{} ({}): {}", r.browser, r.profile, msg));
                }
            }
        }
        total_read = total_read.saturating_add(r.read_count);
        total_inserted = total_inserted.saturating_add(r.inserted_count);
        total_duplicate = total_duplicate.saturating_add(r.duplicate_count);

        details.push(AutoSyncSourceSummary {
            source_id: r.source_id,
            browser: r.browser.clone(),
            profile: r.profile.clone(),
            status: r.status,
            read_count: r.read_count,
            inserted_count: r.inserted_count,
            duplicate_count: r.duplicate_count,
            duration_ms: r.duration_ms,
            error: r
                .error
                .as_deref()
                .map(crate::diagnostics::sanitize_log_text),
        });
    }

    let sources_total = results.len() as u32;
    let status = if sources_total == 0 {
        "skipped"
    } else if sources_failed == 0 {
        "success"
    } else if sources_succeeded > 0 {
        "partial_success"
    } else {
        "failed"
    };

    let error_summary = if errors.is_empty() {
        None
    } else {
        Some(errors.join("；").chars().take(400).collect())
    };
    let all_transient_failed =
        sources_failed > 0 && sources_succeeded == 0 && transient_failures == sources_failed;

    AggregatedRun {
        status,
        sources_total,
        sources_succeeded,
        sources_skipped,
        sources_failed,
        total_read,
        total_inserted,
        total_duplicate,
        error_summary,
        details,
        all_transient_failed,
    }
}

fn resolve_headless_next_run_at(
    enabled: bool,
    existing_next_run_at: Option<i64>,
    finished_at: i64,
    interval_days: u32,
    all_transient_failed: bool,
) -> Option<i64> {
    if !enabled {
        None
    } else if all_transient_failed {
        // Keep existing next_run_at intact so Task Scheduler RestartOnFailure retries
        // are not skipped as SkippedNotDue by the due check.
        existing_next_run_at
    } else {
        Some(compute_next_run_at(finished_at, interval_days))
    }
}

pub fn execute_auto_sync_headless(
    conn: &mut Connection,
    app_dir: &Path,
    temp_dir: &Path,
    exe_path: &Path,
    is_portable: bool,
    trigger: SyncTriggerType,
    force: bool,
) -> AppResult<HeadlessRunOutcome> {
    let now_ms = Utc::now().timestamp_millis();
    let _ = repository::mark_stale_running_runs(conn, now_ms);

    let cfg = repository::get_auto_sync_config(conn)?;
    if !cfg.enabled && !force {
        return Ok(HeadlessRunOutcome::SkippedDisabled);
    }
    if !force
        && trigger == SyncTriggerType::Scheduled
        && cfg.next_run_at.is_some_and(|next| now_ms < next - 300_000)
    {
        return Ok(HeadlessRunOutcome::SkippedNotDue);
    }

    let bound_count = repository::count_bound_enabled_sources(conn)?;
    if bound_count == 0 {
        return Ok(HeadlessRunOutcome::SkippedNoSources);
    }

    let started = Instant::now();
    let run_uuid = Uuid::new_v4().to_string();
    let run_id =
        repository::create_auto_sync_run(conn, &run_uuid, trigger.as_str(), now_ms, bound_count)?;

    let results =
        match batch_sync::sync_all_with_conn(conn, temp_dir, trigger.as_str(), Some(run_id), 3) {
            Ok(r) => r,
            Err(err) => {
                let finished_at = Utc::now().timestamp_millis();
                let duration_ms = started.elapsed().as_millis() as u64;
                let msg = crate::diagnostics::sanitize_log_text(&err.to_string());
                let _ = repository::finish_auto_sync_run(
                    conn,
                    run_id,
                    finished_at,
                    "failed",
                    bound_count,
                    0,
                    0,
                    bound_count,
                    0,
                    0,
                    0,
                    duration_ms,
                    Some(&msg),
                    &[],
                );
                return Err(err);
            }
        };

    let finished_at = Utc::now().timestamp_millis();
    let duration_ms = started.elapsed().as_millis() as u64;
    let agg = aggregate_results(&results);

    repository::finish_auto_sync_run(
        conn,
        run_id,
        finished_at,
        agg.status,
        agg.sources_total,
        agg.sources_succeeded,
        agg.sources_skipped,
        agg.sources_failed,
        agg.total_read,
        agg.total_inserted,
        agg.total_duplicate,
        duration_ms,
        agg.error_summary.as_deref(),
        &agg.details,
    )?;

    let _task_update = scheduler::task_update_guard()?;
    let current_cfg = repository::get_auto_sync_config(conn)?;
    let last_success_at = if agg.status == "success" || agg.status == "partial_success" {
        Some(finished_at)
    } else {
        None
    };
    let next_run_at = resolve_headless_next_run_at(
        current_cfg.enabled,
        current_cfg.next_run_at,
        finished_at,
        current_cfg.interval_days,
        agg.all_transient_failed,
    );
    let exe_str = if exe_path.as_os_str().is_empty() {
        None
    } else {
        Some(exe_path.to_string_lossy().to_string())
    };
    repository::update_auto_sync_timestamps(
        conn,
        Some(finished_at),
        last_success_at,
        next_run_at,
        exe_str.as_deref(),
    )?;
    let _ = repository::prune_old_auto_sync_runs(conn, repository::MAX_STORED_AUTO_SYNC_RUNS);

    if current_cfg.enabled && !agg.all_transient_failed && !exe_path.as_os_str().is_empty() {
        if let Some(next_ts) = next_run_at {
            if let Ok(instance_id) = repository::get_or_create_auto_sync_instance_id(conn) {
                let name = scheduler::task_name(is_portable, &instance_id);
                if let Err(err) = scheduler::create_or_update_task(
                    &name,
                    exe_path,
                    temp_dir,
                    current_cfg.interval_days,
                    next_ts,
                ) {
                    warn!(
                        "Failed to refresh scheduled task after headless sync: {}",
                        err
                    );
                }
            }
        }
    }

    if agg.status == "success" || agg.status == "partial_success" {
        state_file::clear_state_file(app_dir);
    }

    let run = AutoSyncRunItem {
        id: run_id,
        run_uuid,
        trigger_type: trigger.as_str().to_string(),
        started_at: now_ms,
        finished_at: Some(finished_at),
        status: agg.status.to_string(),
        sources_total: agg.sources_total,
        sources_succeeded: agg.sources_succeeded,
        sources_skipped: agg.sources_skipped,
        sources_failed: agg.sources_failed,
        total_read: agg.total_read,
        total_inserted: agg.total_inserted,
        total_duplicate: agg.total_duplicate,
        duration_ms,
        error_summary: agg.error_summary,
        details: agg.details,
    };

    Ok(HeadlessRunOutcome::Completed {
        run,
        all_transient_failed: agg.all_transient_failed,
    })
}

pub fn execute_auto_sync_in_app(
    db: &DbState,
    operation: &VaultOperation,
    trigger: SyncTriggerType,
) -> AppResult<AutoSyncRunItem> {
    let _lease = if trigger == SyncTriggerType::Manual {
        global_sync_coordinator().acquire_or_reject(trigger)?
    } else {
        match global_sync_coordinator().try_acquire(trigger) {
            Some(lease) => lease,
            None => {
                return Err(crate::error::AppError::Sync(
                    "已有同步任务正在执行中，已跳过重复触发".into(),
                ));
            }
        }
    };

    let now_ms = Utc::now().timestamp_millis();
    let (bound_count, run_id, run_uuid) = operation.with_conn(|conn| {
        let _ = repository::mark_stale_running_runs(conn, now_ms);
        let bound_count = repository::count_bound_enabled_sources(conn)?;
        let run_uuid = Uuid::new_v4().to_string();
        let run_id = repository::create_auto_sync_run(
            conn,
            &run_uuid,
            trigger.as_str(),
            now_ms,
            bound_count,
        )?;
        Ok((bound_count, run_id, run_uuid))
    })?;

    let started = Instant::now();
    let results = match batch_sync::sync_all_registered_with_options(
        db,
        operation,
        trigger.as_str(),
        Some(run_id),
        2,
    ) {
        Ok(r) => r,
        Err(err) => {
            let finished_at = Utc::now().timestamp_millis();
            let duration_ms = started.elapsed().as_millis() as u64;
            let msg = crate::diagnostics::sanitize_log_text(&err.to_string());
            let _ = operation.with_conn(|conn| {
                repository::finish_auto_sync_run(
                    conn,
                    run_id,
                    finished_at,
                    "failed",
                    bound_count,
                    0,
                    0,
                    bound_count,
                    0,
                    0,
                    0,
                    duration_ms,
                    Some(&msg),
                    &[],
                )
            });
            return Err(err);
        }
    };

    let finished_at = Utc::now().timestamp_millis();
    let duration_ms = started.elapsed().as_millis() as u64;
    let agg = aggregate_results(&results);

    let last_success_at = if agg.status == "success" || agg.status == "partial_success" {
        Some(finished_at)
    } else {
        None
    };
    let exe_path = std::env::current_exe().ok();
    let exe_str = exe_path
        .as_ref()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    let _task_update = scheduler::task_update_guard()?;
    let (current_cfg, next_run_at, instance_id) = operation.with_conn(|conn| {
        repository::finish_auto_sync_run(
            conn,
            run_id,
            finished_at,
            agg.status,
            agg.sources_total,
            agg.sources_succeeded,
            agg.sources_skipped,
            agg.sources_failed,
            agg.total_read,
            agg.total_inserted,
            agg.total_duplicate,
            duration_ms,
            agg.error_summary.as_deref(),
            &agg.details,
        )?;
        let current_cfg = repository::get_auto_sync_config(conn)?;
        let next_run_at = if current_cfg.enabled {
            Some(compute_next_run_at(finished_at, current_cfg.interval_days))
        } else {
            None
        };
        repository::update_auto_sync_timestamps(
            conn,
            Some(finished_at),
            last_success_at,
            next_run_at,
            if exe_str.is_empty() {
                None
            } else {
                Some(&exe_str)
            },
        )?;
        let _ = repository::prune_old_auto_sync_runs(conn, repository::MAX_STORED_AUTO_SYNC_RUNS);
        let instance_id = repository::get_or_create_auto_sync_instance_id(conn)?;
        Ok((current_cfg, next_run_at, instance_id))
    })?;

    if current_cfg.enabled {
        if let (Some(exe), Some(next_ts)) = (exe_path.as_deref(), next_run_at) {
            let is_portable = exe
                .parent()
                .is_some_and(crate::database::connection::is_portable_install);
            let name = scheduler::task_name(is_portable, &instance_id);
            if let Err(err) = scheduler::create_or_update_task(
                &name,
                exe,
                &db.temp_dir,
                current_cfg.interval_days,
                next_ts,
            ) {
                warn!("Failed to update scheduled task after in-app sync: {}", err);
            }
        }
    }

    if agg.status == "success" || agg.status == "partial_success" {
        state_file::clear_state_file(&db.app_dir);
    }

    Ok(AutoSyncRunItem {
        id: run_id,
        run_uuid,
        trigger_type: trigger.as_str().to_string(),
        started_at: now_ms,
        finished_at: Some(finished_at),
        status: agg.status.to_string(),
        sources_total: agg.sources_total,
        sources_succeeded: agg.sources_succeeded,
        sources_skipped: agg.sources_skipped,
        sources_failed: agg.sources_failed,
        total_read: agg.total_read,
        total_inserted: agg.total_inserted,
        total_duplicate: agg.total_duplicate,
        duration_ms,
        error_summary: agg.error_summary,
        details: agg.details,
    })
}

/// Called after GUI vault unlock to reconcile moved/upgraded EXE paths in Task Scheduler
/// and determine whether a missed or failed scheduled run needs catch-up.
pub fn reconcile_and_check_due_on_startup(
    db: &DbState,
    operation: &VaultOperation,
) -> AppResult<bool> {
    let now_ms = Utc::now().timestamp_millis();
    let _task_update = scheduler::task_update_guard()?;
    let (cfg, instance_id, last_run) = operation.with_conn(|conn| {
        let _ = repository::mark_stale_running_runs(conn, now_ms);
        let cfg = repository::get_auto_sync_config(conn)?;
        let id = repository::get_or_create_auto_sync_instance_id(conn)?;
        let latest = repository::list_auto_sync_runs(conn, 1)?.into_iter().next();
        Ok((cfg, id, latest))
    })?;

    if !cfg.enabled {
        return Ok(false);
    }

    if let Ok(exe) = std::env::current_exe() {
        let is_portable = exe
            .parent()
            .is_some_and(crate::database::connection::is_portable_install);
        let name = scheduler::task_name(is_portable, &instance_id);
        let (health, _) = scheduler::query_task_health(&name, &exe, cfg.interval_days);
        if matches!(
            health,
            SchedulerHealthStatus::Missing | SchedulerHealthStatus::OutdatedPath
        ) {
            let boundary = cfg
                .next_run_at
                .unwrap_or_else(|| compute_next_run_at(now_ms, cfg.interval_days));
            if let Err(err) = scheduler::create_or_update_task(
                &name,
                &exe,
                &db.temp_dir,
                cfg.interval_days,
                boundary,
            ) {
                warn!("Auto-repair of scheduled task on startup failed: {}", err);
            } else {
                info!("Reconciled Windows scheduled task path/config on startup");
            }
        }
    }

    let is_due = cfg.next_run_at.is_some_and(|next| now_ms >= next);
    let last_failed = last_run
        .as_ref()
        .is_some_and(|r| r.status == "failed" && now_ms - r.started_at > 300_000);
    let had_headless_block =
        state_file::read_state_file(&db.app_dir).is_some_and(|s| s.last_exit_code != 0);

    Ok(is_due || last_failed || had_headless_block)
}

pub fn check_due_during_tray(db: &DbState) -> bool {
    if !db.vault.is_unlocked() {
        return false;
    }
    let Ok(op) = db.begin_operation() else {
        return false;
    };
    let now_ms = Utc::now().timestamp_millis();
    let due = op
        .with_conn(|conn| {
            let cfg = repository::get_auto_sync_config(conn)?;
            Ok(cfg.enabled && cfg.next_run_at.is_some_and(|next| now_ms >= next))
        })
        .unwrap_or(false);
    if !due {
        return false;
    }
    match execute_auto_sync_in_app(db, &op, SyncTriggerType::TrayDue) {
        Ok(run) => {
            info!(
                run_id = run.id,
                inserted = run.total_inserted,
                "Completed due auto-sync while app running in background/tray"
            );
            true
        }
        Err(err) => {
            warn!("Tray due auto-sync skipped/failed: {}", err);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::migrations::run_migrations;
    use crate::history::unix_millis_to_webkit_micros;

    #[test]
    fn headless_sync_performs_incremental_archive_preserves_cleared_history_and_isolates_missing_source(
    ) {
        let root =
            std::env::temp_dir().join(format!("browsory_auto_sync_runner_{}", Uuid::new_v4()));
        let app_dir = root.join("app");
        let temp_dir = app_dir.join("temp");
        std::fs::create_dir_all(&temp_dir).unwrap();

        let browser_dir = root.join("chrome");
        std::fs::create_dir_all(&browser_dir).unwrap();
        let history_path = browser_dir.join("History");
        let missing_path = browser_dir.join("MissingHistory");

        let browser_conn = Connection::open(&history_path).unwrap();
        browser_conn
            .query_row("PRAGMA journal_mode=WAL", [], |r| r.get::<_, String>(0))
            .unwrap();
        browser_conn
            .query_row("PRAGMA wal_autocheckpoint=0", [], |r| r.get::<_, i64>(0))
            .unwrap();
        let t1 = Utc::now().timestamp_millis() - 10_000;
        browser_conn
            .execute_batch(&format!(
                "CREATE TABLE urls(id INTEGER PRIMARY KEY, url TEXT, title TEXT);
                 CREATE TABLE visits(id INTEGER PRIMARY KEY, url INTEGER, visit_time INTEGER, from_visit INTEGER, transition INTEGER, visit_duration INTEGER);
                 INSERT INTO urls VALUES(1, 'https://auto-sync-one.test/', 'Page One');
                 INSERT INTO visits VALUES(1, 1, {}, 0, 1, 0);",
                unix_millis_to_webkit_micros(t1)
            ))
            .unwrap();

        let mut archive = Connection::open_in_memory().unwrap();
        run_migrations(&mut archive).unwrap();
        let s1 = repository::get_or_create_source(
            &archive,
            "chrome",
            "Default",
            "auto",
            &history_path.to_string_lossy(),
        )
        .unwrap();

        let fake_exe = root.join("Browsory.exe");
        // 1. Disabled & not forced -> SkippedDisabled
        let skipped = execute_auto_sync_headless(
            &mut archive,
            &app_dir,
            &temp_dir,
            &fake_exe,
            false,
            SyncTriggerType::Scheduled,
            false,
        )
        .unwrap();
        assert!(matches!(skipped, HeadlessRunOutcome::SkippedDisabled));

        // 2. Enable with next_run_at in future -> SkippedNotDue unless forced
        let now = Utc::now().timestamp_millis();
        repository::set_auto_sync_config(&archive, true, 30, Some(now + 10 * MILLIS_PER_DAY))
            .unwrap();
        let not_due = execute_auto_sync_headless(
            &mut archive,
            &app_dir,
            &temp_dir,
            &fake_exe,
            false,
            SyncTriggerType::Scheduled,
            false,
        )
        .unwrap();
        assert!(matches!(not_due, HeadlessRunOutcome::SkippedNotDue));

        // 3. Forced initial sync while browser WAL is live -> archives 1 visit
        let first = execute_auto_sync_headless(
            &mut archive,
            &app_dir,
            &temp_dir,
            &fake_exe,
            false,
            SyncTriggerType::Scheduled,
            true,
        )
        .unwrap();
        match first {
            HeadlessRunOutcome::Completed { run, .. } => {
                assert_eq!(run.status, "success");
                assert_eq!(run.total_inserted, 1);
            }
            _ => panic!("expected completed run"),
        }

        // 4. Add a second visit in WAL and run incremental sync -> archives only +1 new visit
        let t2 = Utc::now().timestamp_millis();
        browser_conn
            .execute(
                "INSERT INTO urls VALUES(2, 'https://auto-sync-two.test/', 'Page Two')",
                [],
            )
            .unwrap();
        browser_conn
            .execute(
                "INSERT INTO visits VALUES(2, 2, ?1, 0, 1, 0)",
                [unix_millis_to_webkit_micros(t2)],
            )
            .unwrap();

        let second = execute_auto_sync_headless(
            &mut archive,
            &app_dir,
            &temp_dir,
            &fake_exe,
            false,
            SyncTriggerType::Scheduled,
            true,
        )
        .unwrap();
        match second {
            HeadlessRunOutcome::Completed { run, .. } => {
                assert_eq!(run.status, "success");
                assert_eq!(run.total_inserted, 1);
                assert_eq!(run.total_duplicate, 1);
            }
            _ => panic!("expected completed run"),
        }

        // 5. Clear browser history + add a missing source -> existing 2 archived visits remain intact
        browser_conn
            .execute_batch("DELETE FROM visits; DELETE FROM urls;")
            .unwrap();
        let _s2 = repository::get_or_create_source(
            &archive,
            "edge",
            "Missing",
            "auto",
            &missing_path.to_string_lossy(),
        )
        .unwrap();
        // Force s1 mtime check to re-read by resetting last_sync_at slightly behind
        archive
            .execute(
                "UPDATE sources SET last_sync_at = ?1 WHERE id = ?2",
                [t1 - 5000, s1.id],
            )
            .unwrap();
        let third = execute_auto_sync_headless(
            &mut archive,
            &app_dir,
            &temp_dir,
            &fake_exe,
            false,
            SyncTriggerType::Scheduled,
            true,
        )
        .unwrap();
        match third {
            HeadlessRunOutcome::Completed { run, .. } => {
                assert_eq!(run.status, "partial_success");
                assert_eq!(run.sources_succeeded, 1);
                assert_eq!(run.sources_failed, 1);
            }
            _ => panic!("expected completed run"),
        }
        let archived_visits: i64 = archive
            .query_row("SELECT COUNT(*) FROM visits", [], |r| r.get(0))
            .unwrap();
        assert_eq!(archived_visits, 2);

        drop(browser_conn);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn transient_failure_preserves_next_run_at_for_retry_while_non_transient_advances() {
        let transient = aggregate_results(&[SyncResult {
            source_id: 1,
            browser: "chrome".into(),
            profile: "Default".into(),
            read_count: 0,
            inserted_count: 0,
            duplicate_count: 0,
            failed_count: 0,
            duration_ms: 5,
            status: SourceSyncStatus::Failed,
            error: Some("读取大体积浏览器历史库期间源文件发生变更，请稍后重试".into()),
        }]);
        assert!(transient.all_transient_failed);
        assert_eq!(transient.status, "failed");

        let due_ts = 1_700_000_000_000i64;
        let finished_ts = due_ts + 60_000;
        assert_eq!(
            resolve_headless_next_run_at(
                true,
                Some(due_ts),
                finished_ts,
                30,
                transient.all_transient_failed
            ),
            Some(due_ts),
        );

        let missing = aggregate_results(&[SyncResult {
            source_id: 2,
            browser: "edge".into(),
            profile: "Default".into(),
            read_count: 0,
            inserted_count: 0,
            duplicate_count: 0,
            failed_count: 0,
            duration_ms: 1,
            status: SourceSyncStatus::Failed,
            error: Some("源文件不存在".into()),
        }]);
        assert!(!missing.all_transient_failed);
        assert_eq!(
            resolve_headless_next_run_at(
                true,
                Some(due_ts),
                finished_ts,
                30,
                missing.all_transient_failed
            ),
            Some(compute_next_run_at(finished_ts, 30)),
        );
    }
}
