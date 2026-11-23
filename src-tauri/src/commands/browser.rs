use tauri::State;

use crate::browser::{detect_all_browsers, get_all_adapters, DiscoveredProfile};
use crate::database::repository::{
    get_or_create_source, list_sources, toggle_source_enabled as repo_toggle_source_enabled,
};
use crate::database::DbState;
use crate::error::AppResult;
use crate::process::terminate_and_wait;

#[tauri::command]
pub async fn scan_browsers(db: State<'_, DbState>) -> AppResult<Vec<DiscoveredProfile>> {
    let operation = db.begin_operation()?;
    super::blocking::run(move || {
        operation.check()?;

        let result = (|| {
            let mut discovered = detect_all_browsers();
            if discovered.is_empty() {
                return Ok(discovered);
            }

            // Match existing sources via concurrent read pool first so Sync Center does not
            // stall behind write transactions; acquire the write lock only for new profiles.
            let existing = operation.with_read_conn(list_sources)?;
            let mut known: std::collections::HashMap<(&str, &str, &str), (i64, bool)> =
                std::collections::HashMap::with_capacity(existing.len());
            for src in &existing {
                known.insert(
                    (
                        src.browser.as_str(),
                        src.profile.as_str(),
                        src.history_path.as_str(),
                    ),
                    (src.id, src.enabled),
                );
            }

            let mut missing_indices = Vec::new();
            let mut enabled_watch_paths = Vec::new();
            for (idx, p) in discovered.iter_mut().enumerate() {
                if let Some(&(id, enabled)) = known.get(&(
                    p.browser.as_str(),
                    p.profile_id.as_str(),
                    p.history_path.as_str(),
                )) {
                    p.source_id = Some(id);
                    if enabled {
                        enabled_watch_paths.push(p.history_path.clone());
                    }
                } else {
                    missing_indices.push(idx);
                }
            }

            if !missing_indices.is_empty() {
                operation.with_conn(|conn| {
                    for idx in missing_indices {
                        let p = &mut discovered[idx];
                        if let Ok(src) = get_or_create_source(
                            conn,
                            &p.browser,
                            &p.profile_id,
                            "auto",
                            &p.history_path,
                        ) {
                            p.source_id = Some(src.id);
                            if src.enabled {
                                enabled_watch_paths.push(p.history_path.clone());
                            }
                        }
                    }
                    Ok(())
                })?;
            }

            for history_path in enabled_watch_paths {
                let path = std::path::Path::new(&history_path);
                if let Some(parent) = path.parent() {
                    crate::history::watcher::watch_history_directory(parent);
                }
            }

            Ok(discovered)
        })();
        operation.check()?;
        result
    })
    .await?
}

#[tauri::command]
pub async fn check_browser_running(browser: String, db: State<'_, DbState>) -> AppResult<bool> {
    let operation = db.begin_operation()?;
    super::blocking::run(move || {
        operation.check()?;

        for adapter in get_all_adapters() {
            if adapter.browser_id().eq_ignore_ascii_case(&browser)
                || adapter.display_name().eq_ignore_ascii_case(&browser)
            {
                let running = adapter.is_running();
                operation.check()?;
                return Ok(running);
            }
        }
        operation.check()?;
        Ok(false)
    })
    .await?
}

#[tauri::command]
pub async fn kill_browser(
    browser: String,
    force: bool,
    db: State<'_, DbState>,
) -> Result<(), String> {
    let _operation = db.begin_operation().map_err(|e| e.to_string())?;
    super::blocking::run(move || {
        _operation.check().map_err(|error| error.to_string())?;

        for adapter in get_all_adapters() {
            if adapter.browser_id().eq_ignore_ascii_case(&browser)
                || adapter.display_name().eq_ignore_ascii_case(&browser)
            {
                return terminate_and_wait(adapter.process_name(), force, 3500);
            }
        }
        Err(format!("Unknown browser identifier: {}", browser))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn toggle_source_enabled(
    source_id: i64,
    enabled: bool,
    db: State<'_, DbState>,
) -> AppResult<()> {
    let operation = db.begin_operation()?;
    super::blocking::run(move || {
        operation.check()?;

        let result = (|| {
            let conn_arc = operation.connection()?;
            let conn = conn_arc.lock()?;
            let source_opt = crate::database::repository::get_source_by_id(&conn, source_id)?;
            repo_toggle_source_enabled(&conn, source_id, enabled)?;

            if let Some(src) = source_opt {
                let path = std::path::Path::new(&src.history_path);
                if let Some(parent) = path.parent() {
                    if enabled {
                        crate::history::watcher::watch_history_directory(parent);
                    } else {
                        let remaining_in_parent = crate::database::repository::list_sources(&conn)?
                            .into_iter()
                            .any(|other| {
                                other.enabled
                                    && other.id != source_id
                                    && std::path::Path::new(&other.history_path).parent()
                                        == Some(parent)
                            });
                        if !remaining_in_parent {
                            crate::history::watcher::unwatch_history_directory(parent);
                        }
                    }
                }
            }

            Ok(())
        })();
        operation.check()?;
        result
    })
    .await?
}

#[tauri::command]
pub async fn open_path_in_folder(path: String, db: State<'_, DbState>) -> Result<(), String> {
    let _operation = db.begin_operation().map_err(|e| e.to_string())?;
    let managed_dirs = [
        db.app_dir.clone(),
        db.temp_dir.clone(),
        db.logs_dir.clone(),
        db.app_dir.join("backups"),
    ];
    super::blocking::run(move || {
        _operation.check().map_err(|error| error.to_string())?;

        let raw_path = std::path::PathBuf::from(&path);
        let mut target = raw_path.clone();
        if !target.exists() {
            if managed_dirs.iter().any(|dir| dir == &raw_path) {
                std::fs::create_dir_all(&target).map_err(|e| format!("无法创建应用目录: {}", e))?;
            } else if let Some(parent) = raw_path.parent().filter(|parent| parent.is_dir()) {
                target = parent.to_path_buf();
            } else {
                return Err("指定的文件或父级目录不存在".to_string());
            }
        }

        #[cfg(target_os = "windows")]
        {
            let win_target = target.to_string_lossy().replace('/', "\\");
            if target.is_dir() {
                std::process::Command::new("explorer")
                    .arg(&win_target)
                    .spawn()
                    .map_err(|e| format!("无法打开资源管理器: {}", e))?;
            } else {
                std::process::Command::new("explorer")
                    .args(["/select,", &win_target])
                    .spawn()
                    .map_err(|e| format!("无法打开资源管理器: {}", e))?;
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            let open_target = if target.is_dir() {
                target.as_path()
            } else {
                target.parent().unwrap_or(target.as_path())
            };
            let _ = open::that(open_target);
        }

        Ok(())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn import_history_file(
    file_path: String,
    browser_name: Option<String>,
    profile_name: Option<String>,
    db: State<'_, DbState>,
) -> AppResult<crate::database::models::SyncResult> {
    let operation = db.begin_operation()?;
    let db = db.inner().clone();
    super::blocking::run(move || {
        operation.check()?;

        let result = (|| {
            let path = std::path::PathBuf::from(&file_path);
            if !path.exists() {
                return Err(crate::error::AppError::Browser(
                    "历史数据库文件不存在".to_string(),
                ));
            }

            let b_name = browser_name.unwrap_or_else(|| "custom".to_string());
            let p_name = profile_name.unwrap_or_else(|| {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Imported")
                    .to_string()
            });

            let source = {
                let conn_arc = operation.connection()?;
                let conn = conn_arc.lock()?;
                get_or_create_source(&conn, &b_name, &p_name, "manual", &file_path)?
            };

            crate::history::sync::sync_registered_source(&db, &operation, &source, &path)
        })();
        operation.check()?;
        result
    })
    .await?
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct BatchImportFileResult {
    pub file_path: String,
    pub file_name: String,
    pub success: bool,
    pub inserted_count: u32,
    pub duplicate_count: u32,
    pub failed_count: u32,
    pub error: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct BatchImportResult {
    pub total_files: usize,
    pub successful_files: usize,
    pub failed_files: usize,
    pub total_inserted: u32,
    pub total_duplicate: u32,
    pub results: Vec<BatchImportFileResult>,
}

#[tauri::command]
pub async fn import_history_batch(
    file_paths: Vec<String>,
    db: State<'_, DbState>,
) -> Result<BatchImportResult, String> {
    let operation = db.begin_operation().map_err(|e| e.to_string())?;
    let db = db.inner().clone();
    super::blocking::run(move || {
operation.check().map_err(|error| error.to_string())?;

    let result = (|| {
        let mut file_results = Vec::new();
        let mut total_inserted: u32 = 0;
        let mut total_duplicate: u32 = 0;
        let mut successful_files = 0;
        let mut failed_files = 0;

        for file_path in &file_paths {
            operation.check().map_err(|e| e.to_string())?;
            let path = std::path::PathBuf::from(file_path);
            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("history_file")
                .to_string();

            if !path.exists() {
                failed_files += 1;
                file_results.push(BatchImportFileResult {
                    file_path: file_path.clone(),
                    file_name,
                    success: false,
                    inserted_count: 0,
                    duplicate_count: 0,
                    failed_count: 0,
                    error: Some("文件不存在".to_string()),
                });
                continue;
            }

            // Quick probe: handle both unlocked and exclusively locked open-browser History DBs
            let is_valid_sqlite = crate::history::snapshot::probe_supported_history_db(&path);

            if !is_valid_sqlite {
                let is_json = path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("json"));

                let try_takeout = is_json || path.extension().is_none();
                let mut takeout_handled = false;

                if try_takeout {
                    let takeout_res = {
                        let probe = operation.cancellation_probe();
                        let prepared = crate::history::takeout::prepare_google_takeout_file_checked(&path, probe.clone());
                        operation.check().map_err(|e| e.to_string())?;
                        match prepared {
                        Ok(prepared) => {
                        let conn_arc = operation.connection().map_err(|e| e.to_string())?;
                        let mut conn = conn_arc.lock().map_err(|e| e.to_string())?;
                        crate::history::takeout::commit_google_takeout_file(&mut conn, prepared, probe)
                        },
                        Err(error) => Err(error),
                        }
                    };

                    match takeout_res {
                        Ok(summary) => {
                            successful_files += 1;
                            total_inserted += summary.visits_inserted as u32;
                            total_duplicate += summary.duplicates_skipped as u32;
                            file_results.push(BatchImportFileResult {
                                file_path: file_path.clone(),
                                file_name: file_name.clone(),
                                success: true,
                                inserted_count: summary.visits_inserted as u32,
                                duplicate_count: summary.duplicates_skipped as u32,
                                failed_count: summary.invalid_records_skipped as u32,
                                error: None,
                            });
                            takeout_handled = true;
                        }
                        Err(e) if is_json => {
                            failed_files += 1;
                            file_results.push(BatchImportFileResult {
                                file_path: file_path.clone(),
                                file_name: file_name.clone(),
                                success: false,
                                inserted_count: 0,
                                duplicate_count: 0,
                                failed_count: 0,
                                error: Some(format!("Google Takeout 归档解析失败: {}", e)),
                            });
                            takeout_handled = true;
                        }
                        Err(_) => {}
                    }
                }

                if takeout_handled {
                    continue;
                }

                failed_files += 1;
                file_results.push(BatchImportFileResult {
                file_path: file_path.clone(),
                file_name,
                success: false,
                inserted_count: 0,
                duplicate_count: 0,
                failed_count: 0,
                error: Some("不是有效的 Chromium 或 Firefox 历史记录数据库，亦非支持的 Google Takeout JSON 归档".to_string()),
            });
                continue;
            }

            let b_name = "custom".to_string();
            let p_name = file_name.clone();

            let source = {
                let conn_arc = operation.connection().map_err(|e| e.to_string())?;
                let conn = conn_arc.lock().map_err(|e| e.to_string())?;
                match get_or_create_source(&conn, &b_name, &p_name, "manual", file_path) {
                    Ok(s) => s,
                    Err(e) => {
                        failed_files += 1;
                        file_results.push(BatchImportFileResult {
                            file_path: file_path.clone(),
                            file_name,
                            success: false,
                            inserted_count: 0,
                            duplicate_count: 0,
                            failed_count: 0,
                            error: Some(format!("创建/获取数据源失败: {}", e)),
                        });
                        continue;
                    }
                }
            };

            let sync_res =
                crate::history::sync::sync_registered_source(&db, &operation, &source, &path);

            match sync_res {
                Ok(res) => {
                    successful_files += 1;
                    total_inserted += res.inserted_count;
                    total_duplicate += res.duplicate_count;
                    file_results.push(BatchImportFileResult {
                        file_path: file_path.clone(),
                        file_name,
                        success: true,
                        inserted_count: res.inserted_count,
                        duplicate_count: res.duplicate_count,
                        failed_count: res.failed_count,
                        error: None,
                    });
                }
                Err(e) => {
                    failed_files += 1;
                    file_results.push(BatchImportFileResult {
                        file_path: file_path.clone(),
                        file_name,
                        success: false,
                        inserted_count: 0,
                        duplicate_count: 0,
                        failed_count: 0,
                        error: Some(format!("解析同步失败: {}", e)),
                    });
                }
            }
        }

        Ok(BatchImportResult {
            total_files: file_paths.len(),
            successful_files,
            failed_files,
            total_inserted,
            total_duplicate,
            results: file_results,
        })
    })();
    operation.check().map_err(|e| e.to_string())?;
    result

}).await.map_err(|error| error.to_string())?
}
