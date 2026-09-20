use tauri::State;

use crate::browser::{detect_all_browsers, get_all_adapters, DiscoveredProfile};
use crate::database::repository::{
    get_or_create_source, toggle_source_enabled as repo_toggle_source_enabled,
};
use crate::database::DbState;
use crate::error::AppResult;
use crate::process::terminate_and_wait;

#[tauri::command]
pub fn scan_browsers(db: State<'_, DbState>) -> AppResult<Vec<DiscoveredProfile>> {
    let mut discovered = detect_all_browsers();

    // Persist discovered profiles into database and attach source_id
    let conn = db.conn.lock().unwrap();
    for p in &mut discovered {
        if let Ok(src) =
            get_or_create_source(&conn, &p.browser, &p.profile_id, "auto", &p.history_path)
        {
            p.source_id = Some(src.id);
            if src.enabled {
                let path = std::path::Path::new(&p.history_path);
                if let Some(parent) = path.parent() {
                    crate::history::watcher::watch_history_directory(parent);
                }
            }
        }
    }

    Ok(discovered)
}

#[tauri::command]
pub fn check_browser_running(browser: String) -> bool {
    for adapter in get_all_adapters() {
        if adapter.browser_id().eq_ignore_ascii_case(&browser)
            || adapter.display_name().eq_ignore_ascii_case(&browser)
        {
            return adapter.is_running();
        }
    }
    false
}

#[tauri::command]
pub fn kill_browser(browser: String, force: bool) -> Result<(), String> {
    for adapter in get_all_adapters() {
        if adapter.browser_id().eq_ignore_ascii_case(&browser)
            || adapter.display_name().eq_ignore_ascii_case(&browser)
        {
            return terminate_and_wait(adapter.process_name(), force, 3500);
        }
    }
    Err(format!("Unknown browser identifier: {}", browser))
}

#[tauri::command]
pub fn toggle_source_enabled(
    source_id: i64,
    enabled: bool,
    db: State<'_, DbState>,
) -> AppResult<()> {
    let conn = db.conn.lock().unwrap();
    let source_opt = crate::database::repository::get_source_by_id(&conn, source_id)?;
    repo_toggle_source_enabled(&conn, source_id, enabled)?;

    if let Some(src) = source_opt {
        let path = std::path::Path::new(&src.history_path);
        if let Some(parent) = path.parent() {
            if enabled {
                crate::history::watcher::watch_history_directory(parent);
            } else {
                crate::history::watcher::unwatch_history_directory(parent);
            }
        }
    }

    Ok(())
}

#[tauri::command]
pub fn open_path_in_folder(path: String) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    if !p.exists() {
        // 如果目录尚不存在，尝试自动创建
        if std::fs::create_dir_all(p).is_err() {
            if let Some(parent) = p.parent() {
                if !parent.exists() {
                    return Err("指定的文件或父级目录不存在".to_string());
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        if p.is_dir() {
            std::process::Command::new("explorer")
                .arg(&path)
                .spawn()
                .map_err(|e| format!("无法打开资源管理器: {}", e))?;
        } else {
            std::process::Command::new("explorer")
                .args(["/select,", &path])
                .spawn()
                .map_err(|e| format!("无法打开资源管理器: {}", e))?;
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let target = if p.is_dir() {
            p
        } else {
            p.parent().unwrap_or(p)
        };
        let _ = open::that(target);
    }

    Ok(())
}

#[tauri::command]
pub fn import_history_file(
    file_path: String,
    browser_name: Option<String>,
    profile_name: Option<String>,
    db: State<'_, DbState>,
) -> AppResult<crate::database::models::SyncResult> {
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
        let conn = db.conn.lock().unwrap();
        get_or_create_source(&conn, &b_name, &p_name, "manual", &file_path)?
    };

    let mut conn = db.conn.lock().unwrap();
    crate::history::sync::sync_source_history(
        &mut conn,
        source.id,
        &source.browser,
        &source.profile,
        &path,
        source.last_visit_time,
        &db.temp_dir,
    )
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
pub fn import_history_batch(
    file_paths: Vec<String>,
    db: State<'_, DbState>,
) -> Result<BatchImportResult, String> {
    let mut file_results = Vec::new();
    let mut total_inserted: u32 = 0;
    let mut total_duplicate: u32 = 0;
    let mut successful_files = 0;
    let mut failed_files = 0;

    for file_path in &file_paths {
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

        // Quick probe: try to open read-only with SQLite and check if valid
        let is_valid_sqlite = {
            match rusqlite::Connection::open_with_flags(
                &path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
            ) {
                Ok(conn) => {
                    let has_visits: bool = conn
                        .query_row(
                            "SELECT 1 FROM sqlite_master WHERE type='table' AND (name='visits' OR name='moz_historyvisits')",
                            [],
                            |_| Ok(true),
                        )
                        .unwrap_or(false);
                    has_visits
                }
                Err(_) => false,
            }
        };

        if !is_valid_sqlite {
            let is_json = path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json"));

            let try_takeout = is_json || path.extension().is_none();
            let mut takeout_handled = false;

            if try_takeout {
                let takeout_res = {
                    let mut conn = db.conn.lock().unwrap();
                    crate::history::takeout::import_google_takeout_file(&mut conn, &path)
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
                            failed_count: 0,
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
            let conn = db.conn.lock().unwrap();
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

        let sync_res = {
            let mut conn = db.conn.lock().unwrap();
            crate::history::sync::sync_source_history(
                &mut conn,
                source.id,
                &source.browser,
                &source.profile,
                &path,
                source.last_visit_time,
                &db.temp_dir,
            )
        };

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
}
