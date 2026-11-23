use chrono::NaiveDate;
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

const MAX_STORAGE_SCAN_DEPTH: usize = 64;
const MAX_STORAGE_SCAN_ENTRIES: usize = 100_000;

fn bounded_dir_size_and_count(
    dir: &Path,
    depth: usize,
    entries_seen: &mut usize,
) -> AppResult<(u64, usize)> {
    let Some(metadata) = crate::security::fs_boundary::plain_metadata(dir)? else {
        return Ok((0, 0));
    };
    if !metadata.is_dir() {
        return Ok((0, 0));
    }
    *entries_seen = entries_seen
        .checked_add(1)
        .ok_or_else(|| crate::error::AppError::Security("存储统计项目数量溢出".into()))?;
    if *entries_seen > MAX_STORAGE_SCAN_ENTRIES || depth > MAX_STORAGE_SCAN_DEPTH {
        return Err(crate::error::AppError::Security(
            "存储统计目录超出安全遍历上限".into(),
        ));
    }

    let mut total_size = 0u64;
    let mut file_count = 0usize;
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let metadata = match crate::security::fs_boundary::plain_metadata(&path) {
            Ok(Some(metadata)) => metadata,
            // A link/reparse point is excluded from accounting rather than
            // traversed. Other failures remain visible to the bounded scan.
            Err(crate::error::AppError::Security(_)) => continue,
            Ok(None) => {
                return Err(crate::error::AppError::Security(
                    "存储统计目录项在检查前消失".into(),
                ))
            }
            Err(error) => return Err(error),
        };
        if metadata.is_file() {
            total_size = total_size
                .checked_add(metadata.len())
                .ok_or_else(|| crate::error::AppError::Security("存储统计大小溢出".into()))?;
            file_count = file_count
                .checked_add(1)
                .ok_or_else(|| crate::error::AppError::Security("存储统计文件数溢出".into()))?;
        } else if metadata.is_dir() {
            let (sub_size, sub_count) = bounded_dir_size_and_count(&path, depth + 1, entries_seen)?;
            total_size = total_size
                .checked_add(sub_size)
                .ok_or_else(|| crate::error::AppError::Security("存储统计大小溢出".into()))?;
            file_count = file_count
                .checked_add(sub_count)
                .ok_or_else(|| crate::error::AppError::Security("存储统计文件数溢出".into()))?;
        } else {
            return Err(crate::error::AppError::Security(
                "存储统计拒绝未知文件类型".into(),
            ));
        }
    }
    Ok((total_size, file_count))
}

pub fn dir_size_and_count(dir: &Path) -> (u64, usize) {
    let mut entries_seen = 0;
    bounded_dir_size_and_count(dir, 0, &mut entries_seen).unwrap_or((0, 0))
}

#[tauri::command]
pub async fn get_storage_breakdown(db_state: State<'_, DbState>) -> AppResult<StorageBreakdown> {
    let operation = db_state.begin_operation()?;
    let db_state = db_state.inner().clone();
    super::blocking::run(move || {
        operation.check()?;

        let db_path = db_state.app_dir.join("archive.db");
        let wal_path = db_state.app_dir.join("archive.db-wal");
        let backups_dir = db_state.app_dir.join("backups");

        let database_bytes = crate::security::fs_boundary::plain_metadata(&db_path)
            .ok()
            .flatten()
            .filter(|metadata| metadata.is_file())
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        let wal_bytes = crate::security::fs_boundary::plain_metadata(&wal_path)
            .ok()
            .flatten()
            .filter(|metadata| metadata.is_file())
            .map(|metadata| metadata.len())
            .unwrap_or(0);

        let (backups_bytes, backups_count) = dir_size_and_count(&backups_dir);
        let (logs_bytes, _) = dir_size_and_count(&db_state.logs_dir);
        let (legacy_temp_bytes, _) = dir_size_and_count(&db_state.temp_dir);
        let (encrypted_temp_bytes, _) = dir_size_and_count(&db_state.app_dir.join("snapshot-work"));
        let temp_bytes = legacy_temp_bytes.saturating_add(encrypted_temp_bytes);

        let total_bytes = database_bytes + wal_bytes + backups_bytes + logs_bytes + temp_bytes;

        operation.check()?;
        Ok(StorageBreakdown {
            database_bytes,
            wal_bytes,
            backups_bytes,
            backups_count,
            logs_bytes,
            temp_bytes,
            total_bytes,
        })
    })
    .await?
}

#[tauri::command]
pub async fn clean_storage_cache(db_state: State<'_, DbState>) -> AppResult<StorageCleanResult> {
    let _operation = db_state.begin_operation()?;
    let db_state = db_state.inner().clone();
    super::blocking::run(move || {
        _operation.check()?;

        let mut bytes_freed = 0;
        let mut temp_files_deleted = 0;
        let mut old_logs_deleted = 0;

        // 1. Clean temp directory (including orphan snapshot UUID folders)
        if db_state.temp_dir.exists() {
            let (del_count, freed) = crate::history::snapshot::cleanup_orphan_snapshots(
                &db_state.app_dir,
                &db_state.temp_dir,
            );
            temp_files_deleted += del_count;
            bytes_freed += freed;
        }

        // 2. Clean old rotated logs (> 7 days)
        let logs_are_scoped = crate::security::fs_boundary::plain_metadata(&db_state.app_dir)
            .ok()
            .flatten()
            .filter(|metadata| metadata.is_dir())
            .and_then(|_| {
                crate::security::fs_boundary::plain_metadata(&db_state.logs_dir)
                    .ok()
                    .flatten()
                    .filter(|metadata| metadata.is_dir())
            })
            .and_then(|_| fs::canonicalize(&db_state.app_dir).ok())
            .and_then(|app_dir| {
                fs::canonicalize(&db_state.logs_dir)
                    .ok()
                    .map(|logs| (app_dir, logs))
            })
            .is_some_and(|(app_dir, logs)| logs == app_dir.join("logs"));
        if logs_are_scoped {
            let now = std::time::SystemTime::now();
            let seven_days = std::time::Duration::from_secs(7 * 24 * 3600);
            if let Ok(entries) = fs::read_dir(&db_state.logs_dir) {
                for entry in entries {
                    let Ok(entry) = entry else {
                        continue;
                    };
                    let path = entry.path();
                    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    // Only clean rotated logs (browsory.log.YYYY-MM-DD or browsory-autosync.log.YYYY-MM-DD), keep active logs
                    let date_suffix = file_name
                        .strip_prefix("browsory.log.")
                        .or_else(|| file_name.strip_prefix("browsory-autosync.log."));
                    if date_suffix
                        .filter(|date| date.len() == 10)
                        .and_then(|date| NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
                        .is_some()
                    {
                        let Ok(Some(meta)) = crate::security::fs_boundary::plain_metadata(&path)
                        else {
                            continue;
                        };
                        if !meta.is_file() {
                            continue;
                        }
                        if let Ok(modified) = meta.modified() {
                            if now.duration_since(modified).unwrap_or_default() > seven_days {
                                let len = meta.len();
                                let still_plain_file =
                                    crate::security::fs_boundary::plain_metadata(&path)
                                        .ok()
                                        .flatten()
                                        .is_some_and(|current| current.is_file());
                                if still_plain_file && fs::remove_file(&path).is_ok() {
                                    bytes_freed += len;
                                    old_logs_deleted += 1;
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
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_size_skips_links_without_reading_their_targets() {
        let root =
            std::env::temp_dir().join(format!("browsory_storage_scan_{}", uuid::Uuid::new_v4()));
        let outside = root.join("outside");
        let scanned = root.join("scanned");
        fs::create_dir_all(&outside).unwrap();
        fs::create_dir_all(&scanned).unwrap();
        fs::write(scanned.join("inside"), b"inside").unwrap();
        fs::write(outside.join("secret"), b"must-not-count").unwrap();
        let link = scanned.join("outside");
        #[cfg(unix)]
        let link_result = std::os::unix::fs::symlink(&outside, &link);
        #[cfg(windows)]
        let link_result = std::os::windows::fs::symlink_dir(&outside, &link);

        let result = dir_size_and_count(&scanned);
        assert_eq!(result, (b"inside".len() as u64, 1));

        if link_result.is_ok() {
            #[cfg(windows)]
            fs::remove_dir(&link).unwrap();
            #[cfg(unix)]
            fs::remove_file(&link).unwrap();
        }
        fs::remove_dir_all(root).unwrap();
    }
}
