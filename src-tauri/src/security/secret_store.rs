use keyring::Entry;
use rusqlite::Connection;
use std::fs;
use std::path::Path;
use tracing::{error, info, warn};

use super::dpapi::{dpapi_protect, dpapi_unprotect};

const SERVICE_NAME: &str = "browsory";

/// Saves a secret string into system keyring (or DPAPI file fallback)
pub fn set_secret(key: &str, value: &str, app_dir: &Path) -> Result<(), String> {
    let trimmed = value.trim();

    // 1. Try native OS keyring (Windows Credential Manager)
    match Entry::new(SERVICE_NAME, key) {
        Ok(entry) => {
            if trimmed.is_empty() {
                let _ = entry.delete_credential();
            } else if let Err(e) = entry.set_password(trimmed) {
                warn!("Keyring set_password failed, using DPAPI fallback: {}", e);
                save_secret_fallback(key, trimmed, app_dir)?;
            } else {
                // Keyring success; ensure fallback file is cleaned
                let _ = remove_secret_fallback(key, app_dir);
                return Ok(());
            }
        }
        Err(e) => {
            warn!(
                "Failed to create keyring entry, using DPAPI fallback: {}",
                e
            );
            save_secret_fallback(key, trimmed, app_dir)?;
        }
    }

    Ok(())
}

/// Retrieves a secret string from system keyring (or DPAPI file fallback)
pub fn get_secret(key: &str, app_dir: &Path) -> Result<Option<String>, String> {
    // 1. Try native OS keyring
    if let Ok(entry) = Entry::new(SERVICE_NAME, key) {
        match entry.get_password() {
            Ok(pwd) => return Ok(Some(pwd)),
            Err(keyring::Error::NoEntry) => {
                // Entry doesn't exist in keyring, check fallback file
            }
            Err(e) => {
                warn!(
                    "Keyring get_password failed, checking DPAPI fallback: {}",
                    e
                );
            }
        }
    }

    // 2. Check DPAPI file fallback
    load_secret_fallback(key, app_dir)
}

/// Deletes a secret from both system keyring and DPAPI file fallback
pub fn delete_secret(key: &str, app_dir: &Path) -> Result<(), String> {
    if let Ok(entry) = Entry::new(SERVICE_NAME, key) {
        let _ = entry.delete_credential();
    }
    let _ = remove_secret_fallback(key, app_dir);
    Ok(())
}

fn get_fallback_path(key: &str, app_dir: &Path) -> std::path::PathBuf {
    let secrets_dir = app_dir.join("secrets");
    let _ = fs::create_dir_all(&secrets_dir);
    secrets_dir.join(format!("{}.enc", key))
}

fn save_secret_fallback(key: &str, value: &str, app_dir: &Path) -> Result<(), String> {
    let path = get_fallback_path(key, app_dir);
    if value.is_empty() {
        let _ = fs::remove_file(&path);
        return Ok(());
    }

    let encrypted = dpapi_protect(value.as_bytes())
        .map_err(|e| format!("DPAPI fallback encryption failed: {}", e))?;
    fs::write(&path, encrypted)
        .map_err(|e| format!("Failed to write fallback secret file: {}", e))?;
    Ok(())
}

fn load_secret_fallback(key: &str, app_dir: &Path) -> Result<Option<String>, String> {
    let path = get_fallback_path(key, app_dir);
    if !path.exists() {
        return Ok(None);
    }

    let raw = fs::read(&path).map_err(|e| format!("Failed to read fallback secret file: {}", e))?;
    let decrypted =
        dpapi_unprotect(&raw).map_err(|e| format!("DPAPI fallback decryption failed: {}", e))?;
    let text =
        String::from_utf8(decrypted).map_err(|e| format!("Secret utf8 decode error: {}", e))?;
    Ok(Some(text))
}

fn remove_secret_fallback(key: &str, app_dir: &Path) -> Result<(), String> {
    let path = get_fallback_path(key, app_dir);
    if path.exists() {
        let _ = fs::remove_file(&path);
    }
    Ok(())
}

/// One-time migration: migrates plain text ai_key from settings table into secure store
pub fn migrate_plain_secrets_from_db(conn: &Connection, app_dir: &Path) {
    let query_res: Result<String, _> = conn.query_row(
        "SELECT value FROM settings WHERE key = 'ai_key' LIMIT 1",
        [],
        |row| row.get(0),
    );

    if let Ok(plain_key) = query_res {
        let trimmed = plain_key.trim();
        if !trimmed.is_empty() {
            info!("Migrating existing plain-text ai_key from SQLite into OS Secret Store...");
            if let Err(e) = set_secret("ai_key", trimmed, app_dir) {
                error!("Failed to store secret into OS Secret Store: {}", e);
                return;
            }
        }
        // Permanently wipe plain key from SQLite database
        let _ = conn.execute("DELETE FROM settings WHERE key = 'ai_key'", []);
        info!("Successfully erased plain-text ai_key from settings table.");
    }
}
