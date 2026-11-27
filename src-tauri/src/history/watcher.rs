use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode, DebounceEventResult, Debouncer};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tauri::Emitter;
use tracing::{info, warn};

static APP_EVENTS: OnceLock<tauri::AppHandle> = OnceLock::new();

pub fn set_app_events(app: tauri::AppHandle) {
    let _ = APP_EVENTS.set(app);
}

pub fn emit_app_event(event: &str) {
    if let Some(app) = APP_EVENTS.get() {
        let _ = app.emit(event, ());
    }
}

use std::sync::{Mutex, OnceLock};

use crate::database::repository::list_sources;
use crate::database::DbState;

pub struct HistoryWatcher {
    debouncer: Mutex<Debouncer<notify::RecommendedWatcher>>,
    _stop_flag: Arc<AtomicBool>,
}

static GLOBAL_WATCHER: OnceLock<Arc<HistoryWatcher>> = OnceLock::new();

pub fn watch_history_directory(dir: &Path) {
    if let Some(w) = GLOBAL_WATCHER.get() {
        let _ = w.watch_directory(dir);
    }
}

pub fn unwatch_history_directory(dir: &Path) {
    if let Some(w) = GLOBAL_WATCHER.get() {
        let _ = w.unwatch_directory(dir);
    }
}

pub fn refresh_active_directories(db_state: &DbState) {
    if let Some(w) = GLOBAL_WATCHER.get() {
        let watched_dirs = get_active_history_directories(db_state);
        for dir in &watched_dirs {
            if dir.exists() {
                let _ = w.watch_directory(dir);
            }
        }
        info!(
            "History Watcher 已更新动态监听目录，共注册 {} 个活动源目录",
            watched_dirs.len()
        );
    }
}

impl HistoryWatcher {
    pub fn watch_directory(&self, dir: &Path) -> Result<(), String> {
        if dir.exists() {
            let mut d = self.debouncer.lock().map_err(|e| e.to_string())?;
            d.watcher()
                .watch(dir, RecursiveMode::NonRecursive)
                .map_err(|e| format!("Failed to watch directory {:?}: {}", dir, e))?;
            info!("History Watcher dynamically monitoring: {:?}", dir);
        }
        Ok(())
    }

    pub fn unwatch_directory(&self, dir: &Path) -> Result<(), String> {
        let mut d = self.debouncer.lock().map_err(|e| e.to_string())?;
        let _ = d.watcher().unwatch(dir);
        info!("History Watcher stopped monitoring: {:?}", dir);
        Ok(())
    }
}

/// Starts the intelligent background watcher:
/// 1. Watches the parent directories of active browser History files
/// 2. Debounces writes by 4 seconds (handles rapid browser WAL writes)
/// 3. Runs a 30-minute fallback periodic scan to ensure zero missed history
pub fn start_history_watcher(db_state: DbState) -> Result<Arc<HistoryWatcher>, String> {
    let sync_state = db_state.clone();

    // 1. Setup debounced filesystem watcher
    let mut debouncer =
        new_debouncer(
            Duration::from_secs(4),
            move |res: DebounceEventResult| match res {
                Ok(events) => {
                    let mut should_sync = false;
                    for event in events {
                        let path = &event.path;
                        if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                            if file_name.starts_with("History")
                                || file_name.starts_with("places.sqlite")
                            {
                                should_sync = true;
                                break;
                            }
                        }
                    }

                    if should_sync {
                        trigger_silent_sync(&sync_state);
                    }
                }
                Err(e) => {
                    warn!("History watcher debounce event error: {:?}", e);
                }
            },
        )
        .map_err(|e| format!("Failed to create notify debouncer: {}", e))?;

    // 2. Register all active sources' directories
    let watched_dirs = get_active_history_directories(&db_state);
    for dir in &watched_dirs {
        if dir.exists() {
            if let Err(e) = debouncer.watcher().watch(dir, RecursiveMode::NonRecursive) {
                warn!("Failed to watch browser history directory {:?}: {}", dir, e);
            } else {
                info!("History Watcher actively monitoring: {:?}", dir);
            }
        }
    }

    // 3. Fallback periodic sweep (every 30 minutes)
    let stop_flag = Arc::new(AtomicBool::new(false));
    let periodic_stop = stop_flag.clone();
    let periodic_state = db_state.clone();

    thread::spawn(move || {
        info!("Started periodic fallback archive sweep thread (interval: 30 mins).");

        while !periodic_stop.load(Ordering::Relaxed) {
            // Sleep in small increments so shutdown is responsive
            for _ in 0..180 {
                if periodic_stop.load(Ordering::Relaxed) {
                    return;
                }
                thread::sleep(Duration::from_secs(10));
            }

            info!("Triggering 30-min periodic fallback archive sync...");
            if crate::auto_sync::runner::check_due_during_tray(&periodic_state) {
                emit_app_event("browsory://auto-sync-updated");
                emit_app_event("browsory://sync-status");
            } else {
                trigger_silent_sync(&periodic_state);
            }
        }
    });

    let watcher = Arc::new(HistoryWatcher {
        debouncer: Mutex::new(debouncer),
        _stop_flag: stop_flag,
    });

    let _ = GLOBAL_WATCHER.set(watcher.clone());

    Ok(watcher)
}

fn trigger_silent_sync(db_state: &DbState) {
    if !db_state.vault.is_unlocked() {
        return;
    }

    if crate::history::coordinator::global_sync_coordinator().is_running() {
        return;
    }

    let state = db_state.clone();
    let operation = match state.begin_operation() {
        Ok(operation) => operation,
        Err(_) => {
            return;
        }
    };
    thread::spawn(move || {
        let Some(_lease) = crate::history::coordinator::global_sync_coordinator()
            .try_acquire(crate::database::models::SyncTriggerType::Watcher)
        else {
            return;
        };
        let results = match crate::history::batch_sync::sync_all_registered_with_options(
            &state, &operation, "watcher", None, 1,
        ) {
            Ok(results) => results,
            Err(error) => {
                if operation.check().is_ok() {
                    tracing::warn!("Background sync did not complete: {}", error);
                }
                return;
            }
        };
        let total_inserted: u32 = results.iter().map(|r| r.inserted_count).sum();
        if let Some(app) = APP_EVENTS.get() {
            // Only notify; the frontend re-queries through its current protected IPC session.
            let _ = app.emit("browsory://sync-status", ());
        }
        let failed = results
            .iter()
            .filter(|r| r.status == crate::database::models::SourceSyncStatus::Failed)
            .count();
        if failed > 0 {
            tracing::warn!(
                failed_sources = failed,
                "Background sync has source failures; see Sync Center"
            );
        }

        if total_inserted > 0 {
            info!(
                "Auto-sync successfully archived {} new visits silently.",
                total_inserted
            );
        }
    });
}

fn get_active_history_directories(db_state: &DbState) -> HashSet<PathBuf> {
    let mut dirs = HashSet::new();
    let sources = db_state
        .with_conn(|conn| Ok(list_sources(conn).unwrap_or_default()))
        .unwrap_or_default();

    for s in sources {
        if s.enabled {
            let p = Path::new(&s.history_path);
            if let Some(parent) = p.parent() {
                dirs.insert(parent.to_path_buf());
            }
        }
    }
    dirs
}
