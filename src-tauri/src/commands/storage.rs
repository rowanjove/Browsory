use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use tauri::State;

use crate::database::connection::DbState;
use crate::error::AppResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageBreakdown {
    pub database_bytes: u64,
    pub wal_bytes: u64,
    pub backups_bytes: u64,
    pub backups_count: usize,
    pub logs_bytes: u64,
    pub temp_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageCleanResult {
    pub bytes_freed: u64,
    pub temp_files_deleted: usize,
    pub old_logs_deleted: usize,
}

fn dir_size_and_count(dir: &Path) -> (u64, usize) {
    if !dir.exists() || !dir.is_dir() {
        return (0, 0);
    }
    let mut total_size = 0;
    let mut file_count = 0;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Ok(meta) = fs::metadata(&path) {
                    total_size += meta.len();
                    file_count += 1;
                }
            } else if path.is_dir() {
                let (sub_size, sub_count) = dir_size_and_count(&path);
                total_size += sub_size;
                file_count += sub_count;
            }
        }
    }
    (total_size, file_count)
}

#[tauri::command]
pub async fn get_storage_breakdown(db_state: State<'_, DbState>) -> AppResult<StorageBreakdown> {
    let db_path = db_state.app_dir.join("archive.db");
    let wal_path = db_state.app_dir.join("archive.db-wal");
    let backups_dir = db_state.app_dir.join("backups");

    let database_bytes = fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);
    let wal_bytes = fs::metadata(&wal_path).map(|m| m.len()).unwrap_or(0);

    let (backups_bytes, backups_count) = dir_size_and_count(&backups_dir);
    let (logs_bytes, _) = dir_size_and_count(&db_state.logs_dir);
    let (temp_bytes, _) = dir_size_and_count(&db_state.temp_dir);

    let total_bytes = database_bytes + wal_bytes + backups_bytes + logs_bytes + temp_bytes;

    Ok(StorageBreakdown {
        database_bytes,
        wal_bytes,
        backups_bytes,
        backups_count,
        logs_bytes,
        temp_bytes,
        total_bytes,
    })
}

#[tauri::command]
pub async fn clean_storage_cache(db_state: State<'_, DbState>) -> AppResult<StorageCleanResult> {
    let mut bytes_freed = 0;
    let mut temp_files_deleted = 0;
    let mut old_logs_deleted = 0;

    // 1. Clean temp directory
    if db_state.temp_dir.exists() {
        if let Ok(entries) = fs::read_dir(&db_state.temp_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    let len = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    if fs::remove_file(&path).is_ok() {
                        bytes_freed += len;
                        temp_files_deleted += 1;
                    }
                }
            }
        }
    }

    // 2. Clean old rotated logs (> 7 days)
    if db_state.logs_dir.exists() {
        let now = std::time::SystemTime::now();
        let seven_days = std::time::Duration::from_secs(7 * 24 * 3600);
        if let Ok(entries) = fs::read_dir(&db_state.logs_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                // Only clean rotated logs (browsory.log.YYYY-MM-DD), keep active log
                if file_name.starts_with("browsory.log.") {
                    if let Ok(meta) = fs::metadata(&path) {
                        if let Ok(modified) = meta.modified() {
                            if now.duration_since(modified).unwrap_or_default() > seven_days {
                                let len = meta.len();
                                if fs::remove_file(&path).is_ok() {
                                    bytes_freed += len;
                                    old_logs_deleted += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(StorageCleanResult {
        bytes_freed,
        temp_files_deleted,
        old_logs_deleted,
    })
}
