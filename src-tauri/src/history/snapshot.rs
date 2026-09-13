use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::error::{AppError, AppResult};

pub struct SnapshotGuard {
    pub snapshot_path: PathBuf,
    temp_dir: PathBuf,
}

impl Drop for SnapshotGuard {
    fn drop(&mut self) {
        if self.temp_dir.exists() {
            debug!("Cleaning up temporary snapshot dir: {:?}", self.temp_dir);
            let _ = fs::remove_dir_all(&self.temp_dir);
        }
    }
}

/// Generates an instance fingerprint of a browser database file.
/// Uses creation time and schema characteristics that identify the database instance,
/// deliberately omitting mtime, file size, and the transaction change counter so that
/// normal incremental writes do not trigger an unnecessary full rescan.
pub fn compute_source_db_signature(source_file: &Path) -> String {
    if !source_file.exists() {
        return String::new();
    }

    let mut hasher = Sha256::new();
    if let Ok(meta) = fs::metadata(source_file) {
        // Use file creation time to detect file replacement/recreation
        if let Ok(ctime) = meta.created() {
            if let Ok(dur) = ctime.duration_since(std::time::UNIX_EPOCH) {
                hasher.update(dur.as_millis().to_le_bytes());
            }
        }
    }

    // Read header static fields: Magic string + Page size (0..18), Schema cookie (40..44), Application ID (68..72)
    if let Ok(file) = fs::File::open(source_file) {
        use std::io::Read;
        let mut header = [0u8; 100];
        let mut reader = std::io::BufReader::new(file);
        if let Ok(n) = reader.read(&mut header) {
            if n >= 18 {
                hasher.update(&header[0..18]);
            }
            if n >= 44 {
                hasher.update(&header[40..44]);
            }
            if n >= 72 {
                hasher.update(&header[68..72]);
            }
        }
    }

    format!("{:x}", hasher.finalize())
}

/// Creates a verified consistent snapshot of the Chromium History SQLite database.
/// Performs quick_check validation and retries automatically if the browser wrote to the DB mid-copy.
pub fn create_history_snapshot(
    source_history_file: &Path,
    temp_base_dir: &Path,
) -> AppResult<SnapshotGuard> {
    if !source_history_file.exists() {
        return Err(AppError::Browser(format!(
            "History file does not exist: {:?}",
            source_history_file
        )));
    }

    let mut attempts = 0;
    const MAX_ATTEMPTS: u32 = 3;

    loop {
        attempts += 1;
        let session_id = Uuid::new_v4().to_string();
        let temp_dir = temp_base_dir.join(&session_id);
        fs::create_dir_all(&temp_dir)?;

        let file_name = source_history_file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("History");
        let target_file = temp_dir.join(file_name);

        // Record pre-copy metadata
        let pre_meta = fs::metadata(source_history_file).ok();

        // Copy primary History database
        if let Err(e) = copy_file_safe(source_history_file, &target_file) {
            let _ = fs::remove_dir_all(&temp_dir);
            if attempts < MAX_ATTEMPTS {
                sleep(Duration::from_millis(60));
                continue;
            }
            return Err(e);
        }

        // Also attempt to copy WAL / SHM / Journal files if present
        let wal_file = source_history_file.with_file_name(format!("{}-wal", file_name));
        if wal_file.exists() {
            let _ = copy_file_safe(&wal_file, &temp_dir.join(format!("{}-wal", file_name)));
        }

        let shm_file = source_history_file.with_file_name(format!("{}-shm", file_name));
        if shm_file.exists() {
            let _ = copy_file_safe(&shm_file, &temp_dir.join(format!("{}-shm", file_name)));
        }

        let journal_file = source_history_file.with_file_name(format!("{}-journal", file_name));
        if journal_file.exists() {
            let _ = copy_file_safe(
                &journal_file,
                &temp_dir.join(format!("{}-journal", file_name)),
            );
        }

        // Validate post-copy file stability
        let post_meta = fs::metadata(source_history_file).ok();
        let was_modified_during_copy = match (pre_meta, post_meta) {
            (Some(pre), Some(post)) => {
                pre.len() != post.len() || pre.modified().ok() != post.modified().ok()
            }
            _ => false,
        };

        if was_modified_during_copy {
            let _ = fs::remove_dir_all(&temp_dir);
            if attempts < MAX_ATTEMPTS {
                warn!(
                    "History file was written by browser during copy attempt #{}. Retrying...",
                    attempts
                );
                sleep(Duration::from_millis(80));
                continue;
            }
            return Err(AppError::Other(
                "浏览器正在高频写入历史文件，快照未能获得一致性版本，请稍后重试".to_string(),
            ));
        }

        // Validate snapshot consistency via PRAGMA quick_check
        let is_valid = validate_snapshot_integrity(&target_file);
        if !is_valid {
            let _ = fs::remove_dir_all(&temp_dir);
            if attempts < MAX_ATTEMPTS {
                warn!(
                    "Snapshot quick_check failed on attempt #{}. Retrying...",
                    attempts
                );
                sleep(Duration::from_millis(80));
                continue;
            }
            return Err(AppError::Other(
                "快照数据库完整性校验失败 (quick_check 异常)，已中止导入".to_string(),
            ));
        }

        info!(
            "Created verified snapshot for {:?} at {:?}",
            source_history_file, target_file
        );

        return Ok(SnapshotGuard {
            snapshot_path: target_file,
            temp_dir,
        });
    }
}

fn validate_snapshot_integrity(db_path: &Path) -> bool {
    let conn_res = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    );

    match conn_res {
        Ok(conn) => {
            let check: Result<String, _> = conn.query_row("PRAGMA quick_check", [], |r| r.get(0));
            check.map(|res| res == "ok").unwrap_or(false)
        }
        Err(e) => {
            warn!("Failed to open snapshot for validation: {}", e);
            false
        }
    }
}

fn copy_file_safe(src: &Path, dst: &Path) -> AppResult<()> {
    match fs::copy(src, dst) {
        Ok(_) => Ok(()),
        Err(e) => {
            warn!("Failed to copy file {:?} to {:?}: {}", src, dst, e);
            Err(AppError::Io(e))
        }
    }
}
