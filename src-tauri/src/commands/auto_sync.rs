use chrono::Utc;
use tauri::State;

use crate::auto_sync::{runner, scheduler, state_file};
use crate::database::models::{
    AutoSyncRunItem, AutoSyncStatus, SchedulerHealthStatus, SyncTriggerType,
};
use crate::database::{connection::is_portable_install, repository, DbState};
use crate::error::AppResult;
use crate::security::session::VaultOperation;

fn build_auto_sync_status(db: &DbState, operation: &VaultOperation) -> AppResult<AutoSyncStatus> {
    operation.check()?;
    let (cfg, maybe_instance_id, bound_sources_count, last_run) =
        operation.with_read_conn(|conn| {
            let cfg = repository::get_auto_sync_config(conn)?;
            let instance_id = repository::get_setting(conn, repository::KEY_AUTO_SYNC_INSTANCE_ID)?;
            let bound_sources_count = repository::count_bound_enabled_sources(conn)?;
            let last_run = repository::list_auto_sync_runs(conn, 1)?.into_iter().next();
            Ok((cfg, instance_id, bound_sources_count, last_run))
        })?;
    let instance_id = match maybe_instance_id.filter(|v| !v.trim().is_empty()) {
        Some(id) => id,
        None => operation.with_conn(repository::get_or_create_auto_sync_instance_id)?,
    };

    let exe_path = std::env::current_exe().ok();
    let is_portable = exe_path
        .as_deref()
        .and_then(|p| p.parent())
        .is_some_and(is_portable_install);
    let task_name = scheduler::task_name(is_portable, &instance_id);

    let (scheduler_status, scheduler_error) = if !cfg.enabled {
        (SchedulerHealthStatus::Disabled, None)
    } else if let Some(exe) = exe_path.as_deref() {
        scheduler::query_task_health(&task_name, exe, cfg.interval_days)
    } else {
        (
            SchedulerHealthStatus::Error,
            Some("无法定位当前主程序路径".to_string()),
        )
    };

    let envelope = db.vault.envelope();
    let headless_capable = envelope
        .as_ref()
        .is_some_and(|env| env.has_dpapi_wrap() && env.recovery_key_confirmed);

    let headless_notice = state_file::read_state_file(&db.app_dir)
        .filter(|s| s.last_exit_code != 0)
        .and_then(|s| s.message);

    Ok(AutoSyncStatus {
        enabled: cfg.enabled,
        interval_days: cfg.interval_days,
        last_run_at: cfg.last_run_at,
        last_success_at: cfg.last_success_at,
        next_run_at: cfg.next_run_at,
        bound_sources_count,
        scheduler_status,
        task_name,
        scheduler_error,
        headless_capable,
        is_running: crate::history::coordinator::global_sync_coordinator().is_running(),
        is_portable,
        last_run,
        headless_notice,
    })
}

#[tauri::command]
pub async fn get_auto_sync_status(db: State<'_, DbState>) -> AppResult<AutoSyncStatus> {
    let operation = db.begin_operation()?;
    let db = db.inner().clone();
    super::blocking::run(move || build_auto_sync_status(&db, &operation)).await?
}

#[tauri::command]
pub async fn update_auto_sync_config(
    enabled: bool,
    interval_days: u32,
    db: State<'_, DbState>,
) -> AppResult<AutoSyncStatus> {
    let operation = db.begin_operation()?;
    let db = db.inner().clone();
    let status = super::blocking::run(move || {
        operation.check()?;
        repository::validate_interval_days(interval_days)?;
        let _task_update = scheduler::task_update_guard()?;

        let now_ms = Utc::now().timestamp_millis();
        let (prev_cfg, instance_id) = operation.with_conn(|conn| {
            let prev = repository::get_auto_sync_config(conn)?;
            let id = repository::get_or_create_auto_sync_instance_id(conn)?;
            Ok((prev, id))
        })?;

        let exe_path = std::env::current_exe().map_err(|e| {
            crate::error::AppError::System(format!("无法定位主程序可执行文件路径: {e}"))
        })?;
        let is_portable = exe_path.parent().is_some_and(is_portable_install);
        let task_name = scheduler::task_name(is_portable, &instance_id);

        let next_run_at = if enabled {
            let candidate = prev_cfg
                .last_success_at
                .map(|last| runner::compute_next_run_at(last, interval_days))
                .unwrap_or_else(|| runner::compute_next_run_at(now_ms, interval_days));
            // Avoid scheduling a past boundary that triggers immediately in a tight loop;
            // if overdue, schedule 1 minute from now or let user click Sync Now.
            Some(candidate.max(now_ms + 60_000))
        } else {
            None
        };

        if enabled {
            scheduler::create_or_update_task(
                &task_name,
                &exe_path,
                &db.temp_dir,
                interval_days,
                next_run_at.unwrap_or_else(|| runner::compute_next_run_at(now_ms, interval_days)),
            )?;
        } else {
            scheduler::delete_task(&task_name)?;
        }

        operation.check()?;
        operation.with_conn(|conn| {
            repository::set_auto_sync_config(conn, enabled, interval_days, next_run_at)?;
            let exe_str = exe_path.to_string_lossy().to_string();
            repository::update_auto_sync_timestamps(conn, None, None, None, Some(&exe_str))?;
            Ok(())
        })?;

        build_auto_sync_status(&db, &operation)
    })
    .await??;

    crate::history::watcher::emit_app_event("browsory://auto-sync-updated");
    Ok(status)
}

#[tauri::command]
pub async fn run_auto_sync_now(db: State<'_, DbState>) -> AppResult<AutoSyncRunItem> {
    let operation = db.begin_operation()?;
    let db = db.inner().clone();
    let run = super::blocking::run(move || {
        operation.check()?;
        runner::execute_auto_sync_in_app(&db, &operation, SyncTriggerType::Manual)
    })
    .await??;

    crate::history::watcher::emit_app_event("browsory://sync-status");
    crate::history::watcher::emit_app_event("browsory://auto-sync-updated");
    Ok(run)
}

#[tauri::command]
pub async fn repair_auto_sync_task(db: State<'_, DbState>) -> AppResult<AutoSyncStatus> {
    let operation = db.begin_operation()?;
    let db = db.inner().clone();
    let status = super::blocking::run(move || {
        operation.check()?;
        let _task_update = scheduler::task_update_guard()?;
        let now_ms = Utc::now().timestamp_millis();
        let (cfg, instance_id) = operation.with_conn(|conn| {
            let cfg = repository::get_auto_sync_config(conn)?;
            let id = repository::get_or_create_auto_sync_instance_id(conn)?;
            Ok((cfg, id))
        })?;
        let exe_path = std::env::current_exe().map_err(|e| {
            crate::error::AppError::System(format!("无法定位主程序可执行文件路径: {e}"))
        })?;
        let is_portable = exe_path.parent().is_some_and(is_portable_install);
        let task_name = scheduler::task_name(is_portable, &instance_id);

        if cfg.enabled {
            let boundary = cfg
                .next_run_at
                .filter(|t| *t > now_ms + 60_000)
                .unwrap_or_else(|| runner::compute_next_run_at(now_ms, cfg.interval_days));
            scheduler::create_or_update_task(
                &task_name,
                &exe_path,
                &db.temp_dir,
                cfg.interval_days,
                boundary,
            )?;
            operation.with_conn(|conn| {
                let exe_str = exe_path.to_string_lossy().to_string();
                repository::update_auto_sync_timestamps(
                    conn,
                    None,
                    None,
                    Some(boundary),
                    Some(&exe_str),
                )
            })?;
        } else {
            scheduler::delete_task(&task_name)?;
        }

        build_auto_sync_status(&db, &operation)
    })
    .await??;

    crate::history::watcher::emit_app_event("browsory://auto-sync-updated");
    Ok(status)
}

#[tauri::command]
pub async fn list_auto_sync_runs(
    limit: Option<u32>,
    db: State<'_, DbState>,
) -> AppResult<Vec<AutoSyncRunItem>> {
    let operation = db.begin_operation()?;
    super::blocking::run(move || {
        operation.check()?;
        let l = limit.unwrap_or(10).clamp(1, 50);
        operation.with_read_conn(|conn| repository::list_auto_sync_runs(conn, l))
    })
    .await?
}
