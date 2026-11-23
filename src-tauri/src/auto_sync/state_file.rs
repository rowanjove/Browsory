use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::AppResult;
use crate::security::atomic_file::write_atomic;

const STATE_FILE_NAME: &str = "auto_sync_state.json";
const MAX_STATE_FILE_BYTES: u64 = 16 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeadlessStateFile {
    pub version: u32,
    pub last_headless_attempt_at: i64,
    pub last_headless_outcome: String,
    pub last_exit_code: i32,
    pub message: Option<String>,
    pub exe_path_hash: String,
}

pub fn state_file_path(app_dir: &Path) -> PathBuf {
    app_dir.join(STATE_FILE_NAME)
}

pub fn hash_exe_path(exe_path: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(exe_path.to_string_lossy().as_bytes());
    let full = format!("{:x}", hasher.finalize());
    full.chars().take(16).collect()
}

pub fn read_state_file(app_dir: &Path) -> Option<HeadlessStateFile> {
    let path = state_file_path(app_dir);
    let meta = fs::metadata(&path).ok()?;
    if meta.len() > MAX_STATE_FILE_BYTES {
        return None;
    }
    let content = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn write_state_file(app_dir: &Path, state: &HeadlessStateFile) -> AppResult<()> {
    let path = state_file_path(app_dir);
    let json = serde_json::to_vec_pretty(state)?;
    write_atomic(&path, &json)?;
    Ok(())
}

pub fn clear_state_file(app_dir: &Path) {
    let path = state_file_path(app_dir);
    if path.exists() {
        let _ = fs::remove_file(path);
    }
}
