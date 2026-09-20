use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode, DebounceEventResult, Debouncer};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tracing::{error, info, warn};

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

static IS_AUTO_SYNCING: AtomicBool = AtomicBool::new(false);

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
            trigger_silent_sync(&periodic_state);
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
    if IS_AUTO_SYNCING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        // A sync is already in progress, ignore duplicate trigger
        return;
    }

    let state = db_state.clone();
    thread::spawn(move || {
        let sources = {
            let conn = state.conn.lock().unwrap();
            list_sources(&conn).unwrap_or_default()
        };

        let active_sources: Vec<_> = sources.into_iter().filter(|s| s.enabled).collect();
        let mut total_inserted = 0;

        for source in active_sources {
            let history_path = PathBuf::from(&source.history_path);
            if !history_path.exists() {
                continue;
            }

            // 1. Fast incremental check: if file and WAL unchanged since last sync, skip with 0 disk I/O!
            if !crate::history::sync::is_source_modified_since_last_sync(
                &history_path,
                source.last_sync_at,
            ) {
                continue;
            }

            let start_time = std::time::Instant::now();

            // 2. Prepare snapshot & parse records WITHOUT holding the database lock
            let prepared = match crate::history::sync::prepare_source_records(
                source.id,
                &source.browser,
                &history_path,
                source.last_visit_time,
                source.db_fingerprint.as_deref(),
                &state.temp_dir,
            ) {
                Ok(data) => data,
                Err(e) => {
                    error!(
                        "Auto-sync preparation failed for source #{}: {} [{}]: {}",
                        source.id, source.browser, source.profile, e
                    );
                    continue;
                }
            };

            // 3. Commit records to archive.db holding lock ONLY during this brief transaction
            let commit_res = {
                let mut conn = state.conn.lock().unwrap();
                crate::history::sync::commit_source_records(
                    &mut conn,
                    source.id,
                    &source.browser,
                    &source.profile,
                    source.last_visit_time,
                    prepared,
                    start_time,
                )
            };

            match commit_res {
                Ok(res) => {
                    total_inserted += res.inserted_count;
                }
                Err(e) => {
                    error!(
                        "Auto-sync commit failed for source #{}: {} [{}]: {}",
                        source.id, source.browser, source.profile, e
                    );
                }
            }
        }

        if total_inserted > 0 {
            info!(
                "Auto-sync successfully archived {} new visits silently.",
                total_inserted
            );
        }

        IS_AUTO_SYNCING.store(false, Ordering::SeqCst);
    });
}

fn get_active_history_directories(db_state: &DbState) -> HashSet<PathBuf> {
    let mut dirs = HashSet::new();
    let sources = {
        let conn = db_state.conn.lock().unwrap();
        list_sources(&conn).unwrap_or_default()
    };

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
