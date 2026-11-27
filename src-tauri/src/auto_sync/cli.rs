use chrono::Utc;
use std::ffi::OsString;
use std::sync::Arc;
use tracing::{error, info, warn};

use super::priority::apply_background_priority;
use super::runner::{execute_auto_sync_headless, HeadlessRunOutcome};
use super::state_file::{hash_exe_path, write_state_file, HeadlessStateFile};
use crate::database::connection::{get_app_directories, is_portable_install};
use crate::database::models::SyncTriggerType;
use crate::error::AppError;
use crate::security::directory_lock::{DataDirectoryLock, LockAcquireError};
use crate::security::vault::VaultManager;

pub const EXIT_SUCCESS: i32 = 0;
pub const EXIT_TRANSIENT_RETRYABLE: i32 = 2;
pub const EXIT_VAULT_NOT_HEADLESS_READY: i32 = 3;
pub const EXIT_FATAL: i32 = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadlessCliOptions {
    pub trigger: SyncTriggerType,
    pub force: bool,
}

pub fn parse_headless_args<I>(args: I) -> Option<HeadlessCliOptions>
where
    I: IntoIterator<Item = OsString>,
{
    let mut is_auto_sync = false;
    let mut trigger = SyncTriggerType::Scheduled;
    let mut force = false;

    for raw in args.into_iter().skip(1) {
        let arg = raw.to_string_lossy();
        if arg == "--auto-sync" {
            is_auto_sync = true;
        } else if arg == "--force" {
            force = true;
        } else if let Some(val) = arg.strip_prefix("--trigger=") {
            trigger = match val {
                "manual" => SyncTriggerType::Manual,
                "startup" => SyncTriggerType::Startup,
                _ => SyncTriggerType::Scheduled,
            };
        }
    }

    if is_auto_sync {
        Some(HeadlessCliOptions { trigger, force })
    } else {
        None
    }
}

/// Entry point called from `main.rs` before Tauri/WebView2 is ever touched.
/// Returns `Some(exit_code)` when `--auto-sync` is present, or `None` for normal GUI startup.
pub fn try_run_headless_from_env() -> Option<i32> {
    let opts = parse_headless_args(std::env::args_os())?;
    Some(run_headless_cli(opts))
}

fn record_headless_state(
    app_dir: &std::path::Path,
    exe_path: &std::path::Path,
    outcome: &str,
    exit_code: i32,
    message: Option<String>,
) {
    let state = HeadlessStateFile {
        version: 1,
        last_headless_attempt_at: Utc::now().timestamp_millis(),
        last_headless_outcome: outcome.to_string(),
        last_exit_code: exit_code,
        message,
        exe_path_hash: hash_exe_path(exe_path),
    };
    let _ = write_state_file(app_dir, &state);
}

pub fn run_headless_cli(opts: HeadlessCliOptions) -> i32 {
    apply_background_priority();

    let (app_dir, temp_dir, logs_dir, _db_path) = match get_app_directories() {
        Ok(dirs) => dirs,
        Err(err) => {
            eprintln!("Failed to resolve Browsory directories: {}", err);
            return EXIT_FATAL;
        }
    };

    let _log_guard = crate::logging::setup_headless_logging(&logs_dir);
    let exe_path = std::env::current_exe().unwrap_or_default();
    let is_portable = exe_path.parent().is_some_and(is_portable_install);

    info!(
        trigger = opts.trigger.as_str(),
        force = opts.force,
        is_portable,
        "Starting headless auto-sync worker"
    );

    // Acquire exclusive directory lock (3 attempts x 1s).
    // If held by an active GUI instance, exit cleanly (0) so GUI handles due syncs;
    // distinguish permission/I/O failures as fatal and surface them to the UI.
    let directory_lock = match DataDirectoryLock::acquire_with_retry_classified(
        &app_dir,
        3,
        std::time::Duration::from_secs(1),
    ) {
        Ok(lock) => Arc::new(lock),
        Err(LockAcquireError::AlreadyHeld(err)) => {
            info!(
                "Data directory is currently owned by running Browsory GUI; headless worker yielding cleanly ({})",
                err
            );
            return EXIT_SUCCESS;
        }
        Err(LockAcquireError::Other(err)) => {
            let msg = crate::diagnostics::sanitize_log_text(&err.to_string());
            error!("Failed to acquire data directory lock: {}", msg);
            record_headless_state(&app_dir, &exe_path, "lock_error", EXIT_FATAL, Some(msg));
            return EXIT_FATAL;
        }
    };

    crate::history::record_spool::cleanup_orphans(&app_dir, &directory_lock);
    let _ = crate::history::snapshot::cleanup_orphan_snapshots(&app_dir, &temp_dir);

    let vault = VaultManager::new(app_dir.clone());
    let mut session = match vault.open_headless_dpapi_session() {
        Ok(s) => s,
        Err(err) => {
            let msg = crate::diagnostics::sanitize_log_text(&err.to_string());
            warn!("Headless vault open skipped/blocked: {}", msg);
            let code = match err {
                AppError::VaultLocked(_) | AppError::Security(_) => EXIT_VAULT_NOT_HEADLESS_READY,
                _ => EXIT_FATAL,
            };
            record_headless_state(
                &app_dir,
                &exe_path,
                "skipped_vault_not_ready",
                code,
                Some(msg),
            );
            return code;
        }
    };

    match execute_auto_sync_headless(
        session.connection_mut(),
        &app_dir,
        &temp_dir,
        &exe_path,
        is_portable,
        opts.trigger,
        opts.force,
    ) {
        Ok(HeadlessRunOutcome::Completed {
            run,
            all_transient_failed,
        }) => {
            info!(
                run_id = run.id,
                status = run.status,
                inserted = run.total_inserted,
                duplicate = run.total_duplicate,
                duration_ms = run.duration_ms,
                "Headless auto-sync finished"
            );
            if all_transient_failed {
                record_headless_state(
                    &app_dir,
                    &exe_path,
                    "transient_failed",
                    EXIT_TRANSIENT_RETRYABLE,
                    run.error_summary,
                );
                EXIT_TRANSIENT_RETRYABLE
            } else if run.status == "failed" {
                record_headless_state(&app_dir, &exe_path, "failed", EXIT_FATAL, run.error_summary);
                EXIT_FATAL
            } else {
                EXIT_SUCCESS
            }
        }
        Ok(HeadlessRunOutcome::SkippedDisabled) => {
            info!("Auto-sync is disabled in settings; exiting cleanly");
            EXIT_SUCCESS
        }
        Ok(HeadlessRunOutcome::SkippedNotDue) => {
            info!("Auto-sync is not yet due; exiting cleanly");
            EXIT_SUCCESS
        }
        Ok(HeadlessRunOutcome::SkippedNoSources) => {
            info!("No bound enabled browser sources; exiting cleanly");
            EXIT_SUCCESS
        }
        Err(err) => {
            let msg = crate::diagnostics::sanitize_log_text(&err.to_string());
            error!("Headless auto-sync failed: {}", msg);
            let code = if crate::history::batch_sync::is_transient_sync_error(&err) {
                EXIT_TRANSIENT_RETRYABLE
            } else {
                EXIT_FATAL
            };
            record_headless_state(&app_dir, &exe_path, "failed", code, Some(msg));
            code
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_arg_parser_only_accepts_whitelisted_auto_sync_flags() {
        assert!(parse_headless_args([OsString::from("browsory.exe")]).is_none());
        assert!(
            parse_headless_args([OsString::from("browsory.exe"), OsString::from("--help")])
                .is_none()
        );

        let parsed = parse_headless_args([
            OsString::from("browsory.exe"),
            OsString::from("--auto-sync"),
            OsString::from("--trigger=scheduled"),
            OsString::from("--force"),
        ])
        .unwrap();
        assert_eq!(parsed.trigger, SyncTriggerType::Scheduled);
        assert!(parsed.force);
    }
}
