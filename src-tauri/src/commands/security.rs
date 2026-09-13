use chrono::Utc;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use tauri::State;
use tracing::{error, info};

use crate::database::migrations::run_migrations;
use crate::database::models::{BackupInfo, IntegrityReport, PrivacyRule, SecurityState};
use crate::database::repository;
use crate::database::DbState;
use crate::error::AppResult;
use crate::security::master_key::{
    create_master_key_envelope, rewrap_master_key_envelope, unwrap_master_key_with_pin,
    unwrap_master_key_with_recovery, MasterKeyEnvelope,
};

fn hash_pin_with_salt(pin: &str, salt: &str) -> String {
    let mut current = format!("{}{}", salt, pin);
    for _ in 0..10_000 {
        let mut hasher = Sha256::new();
        hasher.update(current.as_bytes());
        current = format!("{:x}", hasher.finalize());
    }
    current
}

fn calculate_exponential_lockout(new_fails: i64) -> (i64, String) {
    let now = Utc::now().timestamp_millis();
    if new_fails >= 15 {
        (
            now + 30 * 60 * 1000,
            "连续输错达到 15 次，安全锁定 30 分钟".to_string(),
        )
    } else if new_fails >= 10 {
        (
            now + 5 * 60 * 1000,
            "连续输错达到 10 次，安全锁定 5 分钟".to_string(),
        )
    } else if new_fails >= 8 {
        (
            now + 60 * 1000,
            "连续输错达到 8 次，安全锁定 1 分钟".to_string(),
        )
    } else if new_fails >= 5 {
        (
            now + 30 * 1000,
            "连续输错达到 5 次，安全锁定 30 秒".to_string(),
        )
    } else {
        let left = 5 - new_fails;
        (0, format!("PIN 码错误，还剩 {} 次重试机会", left))
    }
}

#[tauri::command]
pub fn get_security_state(db: State<'_, DbState>) -> AppResult<SecurityState> {
    let conn = db.conn.lock().unwrap();
    repository::get_security_state(&conn)
}

#[tauri::command]
pub fn verify_pin(pin: String, db: State<'_, DbState>) -> Result<bool, String> {
    let now = Utc::now().timestamp_millis();
    let conn = db.conn.lock().unwrap();

    let creds = repository::get_security_credentials(&conn).map_err(|e| e.to_string())?;

    if !creds.pin_enabled {
        return Ok(true);
    }

    if creds.locked_until > now {
        let rem = ((creds.locked_until - now + 999) / 1000).max(1);
        return Err(format!("安全保护冷却中，请在 {} 秒后重试", rem));
    }

    // 1. Check if Argon2id Master Key Envelope exists
    let envelope_opt: Option<String> = repository::get_master_key_envelope(&conn).unwrap_or(None);

    let is_valid = if let Some(env_json) = envelope_opt {
        if let Ok(envelope) = serde_json::from_str::<MasterKeyEnvelope>(&env_json) {
            let ok = unwrap_master_key_with_pin(&envelope, &pin).is_ok();
            if ok && (creds.pin_hash.is_some() || creds.pin_salt.is_some()) {
                // Permanently wipe any legacy weak SHA-256 hash
                let _ = repository::update_pin(&conn, None, None);
            }
            ok
        } else {
            false
        }
    } else {
        // Fallback to legacy SHA-256 x 10,000 verification for legacy users
        let salt = creds.pin_salt.unwrap_or_default();
        let expected_hash = creds.pin_hash.unwrap_or_default();
        let legacy_match =
            !expected_hash.is_empty() && hash_pin_with_salt(&pin, &salt) == expected_hash;

        // Auto-upgrade legacy hash to Argon2id envelope upon successful login, then erase weak hash
        if legacy_match {
            info!("Upgrading legacy SHA-256 PIN hash to Argon2id Master Key Envelope...");
            if let Ok((new_envelope, _, _)) = create_master_key_envelope(&pin, now) {
                if let Ok(json) = serde_json::to_string(&new_envelope) {
                    let _ = repository::save_master_key_envelope(&conn, &json, None);
                    let _ = repository::update_pin(&conn, None, None);
                }
            }
        }
        legacy_match
    };

    if is_valid {
        let _ = repository::reset_pin_failures(&conn);
        Ok(true)
    } else {
        let new_fails = creds.failed_attempts + 1;
        let (new_lock, msg) = calculate_exponential_lockout(new_fails);
        let _ = repository::record_pin_failure(&conn, new_fails, new_lock);
        Err(msg)
    }
}

#[tauri::command]
pub fn set_or_change_pin(
    old_pin: Option<String>,
    new_pin: String,
    db: State<'_, DbState>,
) -> Result<Option<String>, String> {
    let trimmed_new = new_pin.trim();
    if trimmed_new.len() < 4
        || trimmed_new.len() > 6
        || !trimmed_new.chars().all(|c| c.is_ascii_digit())
    {
        return Err("PIN 码必须为 4 至 6 位纯数字".to_string());
    }

    let conn = db.conn.lock().unwrap();
    let creds = repository::get_security_credentials(&conn).map_err(|e| e.to_string())?;

    let now = Utc::now().timestamp_millis();
    let existing_envelope_json = repository::get_master_key_envelope(&conn).unwrap_or(None);

    let mut return_recovery_key: Option<String> = None;

    if creds.pin_enabled {
        let old = old_pin.ok_or_else(|| "请输入原 PIN 码以确认身份".to_string())?;

        if let Some(env_json) = existing_envelope_json {
            let mut envelope: MasterKeyEnvelope = serde_json::from_str(&env_json)
                .map_err(|e| format!("解析主密钥信封失败: {}", e))?;

            // Unwrap master key with old PIN
            let master_key = unwrap_master_key_with_pin(&envelope, &old)
                .map_err(|_| "原 PIN 码校验失败，无法修改".to_string())?;

            // Re-wrap envelope with new PIN (underlying Master Key stays constant)
            rewrap_master_key_envelope(&mut envelope, &master_key, trimmed_new, now)?;
            let updated_json = serde_json::to_string(&envelope)
                .map_err(|e| format!("序列化主密钥信封失败: {}", e))?;

            repository::save_master_key_envelope(&conn, &updated_json, None)
                .map_err(|e| e.to_string())?;
        } else {
            // Legacy PIN migration
            let salt = creds.pin_salt.unwrap_or_default();
            let expected = creds.pin_hash.unwrap_or_default();
            if expected.is_empty() || hash_pin_with_salt(&old, &salt) != expected {
                return Err("原 PIN 码校验失败，无法修改".to_string());
            }

            // Create initial Master Key Envelope
            let (envelope, rec_key, _) = create_master_key_envelope(trimmed_new, now)?;
            let env_json = serde_json::to_string(&envelope)
                .map_err(|e| format!("序列化主密钥信封失败: {}", e))?;
            repository::save_master_key_envelope(&conn, &env_json, None)
                .map_err(|e| e.to_string())?;
            return_recovery_key = Some(rec_key);
        }
    } else {
        // Setting PIN for the first time: generate Master Key and Recovery Key
        let (envelope, rec_key, _) = create_master_key_envelope(trimmed_new, now)?;
        let env_json =
            serde_json::to_string(&envelope).map_err(|e| format!("序列化主密钥信封失败: {}", e))?;
        repository::save_master_key_envelope(&conn, &env_json, None).map_err(|e| e.to_string())?;
        return_recovery_key = Some(rec_key);
    }

    // Set pin_enabled = 1, but NEVER store weak SHA-256 hash
    repository::update_pin(&conn, None, None).map_err(|e| e.to_string())?;

    Ok(return_recovery_key)
}

#[tauri::command]
pub fn verify_recovery_key(recovery_key: String, db: State<'_, DbState>) -> Result<bool, String> {
    let conn = db.conn.lock().unwrap();
    let envelope_json = repository::get_master_key_envelope(&conn)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "当前系统未初始化主密钥信封".to_string())?;

    let envelope: MasterKeyEnvelope =
        serde_json::from_str(&envelope_json).map_err(|e| format!("解析主密钥信封失败: {}", e))?;

    unwrap_master_key_with_recovery(&envelope, &recovery_key).map(|_| true)
}

#[tauri::command]
pub fn reset_pin_with_recovery_key(
    recovery_key: String,
    new_pin: String,
    db: State<'_, DbState>,
) -> Result<(), String> {
    let trimmed_new = new_pin.trim();
    if trimmed_new.len() < 4
        || trimmed_new.len() > 6
        || !trimmed_new.chars().all(|c| c.is_ascii_digit())
    {
        return Err("新 PIN 码必须为 4 至 6 位纯数字".to_string());
    }

    let conn = db.conn.lock().unwrap();
    let envelope_json = repository::get_master_key_envelope(&conn)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "当前系统未初始化主密钥信封".to_string())?;

    let mut envelope: MasterKeyEnvelope =
        serde_json::from_str(&envelope_json).map_err(|e| format!("解析主密钥信封失败: {}", e))?;

    let master_key = unwrap_master_key_with_recovery(&envelope, &recovery_key)
        .map_err(|_| "安全恢复密钥校验失败，无法重置 PIN 码".to_string())?;

    let now = Utc::now().timestamp_millis();
    rewrap_master_key_envelope(&mut envelope, &master_key, trimmed_new, now)?;

    let updated_json =
        serde_json::to_string(&envelope).map_err(|e| format!("序列化主密钥信封失败: {}", e))?;
    repository::save_master_key_envelope(&conn, &updated_json, None).map_err(|e| e.to_string())?;

    // Update security table: PIN enabled, but no weak SHA-256 hash stored
    repository::update_pin(&conn, None, None).map_err(|e| e.to_string())?;
    let _ = repository::reset_pin_failures(&conn);

    Ok(())
}

#[tauri::command]
pub fn disable_pin(current_pin: String, db: State<'_, DbState>) -> Result<(), String> {
    let conn = db.conn.lock().unwrap();
    let creds = repository::get_security_credentials(&conn).map_err(|e| e.to_string())?;

    if !creds.pin_enabled {
        return Ok(());
    }

    let envelope_json = repository::get_master_key_envelope(&conn).unwrap_or(None);
    let mut verified = false;

    if let Some(env_json) = envelope_json {
        if let Ok(envelope) = serde_json::from_str::<MasterKeyEnvelope>(&env_json) {
            verified = unwrap_master_key_with_pin(&envelope, &current_pin).is_ok();
        }
    }

    if !verified {
        let salt = creds.pin_salt.unwrap_or_default();
        let expected = creds.pin_hash.unwrap_or_default();
        if expected.is_empty() || hash_pin_with_salt(&current_pin, &salt) != expected {
            return Err("当前 PIN 码错误，无法关闭应用锁".to_string());
        }
    }

    repository::clear_pin(&conn).map_err(|e| e.to_string())?;
    let _ = repository::clear_master_key_envelope(&conn);
    Ok(())
}

#[tauri::command]
pub fn update_security_options(
    auto_lock_minutes: i64,
    lock_on_minimize: bool,
    lock_on_sleep: bool,
    db: State<'_, DbState>,
) -> AppResult<()> {
    let conn = db.conn.lock().unwrap();
    repository::update_security_options(&conn, auto_lock_minutes, lock_on_minimize, lock_on_sleep)
}

// ==========================================
// Privacy Rules Commands
// ==========================================

#[tauri::command]
pub fn list_privacy_rules(db: State<'_, DbState>) -> AppResult<Vec<PrivacyRule>> {
    let conn = db.conn.lock().unwrap();
    repository::list_privacy_rules(&conn)
}

#[tauri::command]
pub fn add_privacy_rule(
    pattern: String,
    rule_type: String,
    db: State<'_, DbState>,
) -> Result<PrivacyRule, String> {
    let trimmed = pattern.trim();
    if trimmed.is_empty() {
        return Err("规则匹配模式不能为空".to_string());
    }
    let normalized_type = rule_type.trim().to_lowercase();
    if !matches!(normalized_type.as_str(), "private" | "hidden") {
        return Err("规则类型必须是 private 或 hidden".to_string());
    }
    let conn = db.conn.lock().unwrap();
    repository::add_privacy_rule(&conn, trimmed, &normalized_type).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn toggle_privacy_rule(id: i64, enabled: bool, db: State<'_, DbState>) -> AppResult<()> {
    let conn = db.conn.lock().unwrap();
    repository::toggle_privacy_rule(&conn, id, enabled)
}

#[tauri::command]
pub fn delete_privacy_rule(id: i64, db: State<'_, DbState>) -> AppResult<()> {
    let conn = db.conn.lock().unwrap();
    repository::delete_privacy_rule(&conn, id)
}

// ==========================================
// Database Maintenance, Backup & Restore Commands
// ==========================================

#[tauri::command]
pub fn create_database_backup(
    notes: Option<String>,
    db: State<'_, DbState>,
) -> Result<BackupInfo, String> {
    let backups_dir = db.app_dir.join("backups");
    fs::create_dir_all(&backups_dir).map_err(|e| format!("创建备份目录失败: {}", e))?;

    let timestamp_str = Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let file_name = format!("archive_backup_{}.db", timestamp_str);
    let target_path = backups_dir.join(&file_name);

    {
        let conn = db.conn.lock().unwrap();
        let mut dest_conn = rusqlite::Connection::open(&target_path)
            .map_err(|e| format!("打开目标备份文件失败: {}", e))?;

        let backup = rusqlite::backup::Backup::new(&conn, &mut dest_conn)
            .map_err(|e| format!("初始化 SQLite 在线备份失败: {}", e))?;

        backup
            .run_to_completion(100, std::time::Duration::from_millis(5), None)
            .map_err(|e| format!("执行在线备份失败: {}", e))?;
    }

    let file_size_bytes = fs::metadata(&target_path).map(|m| m.len()).unwrap_or(0);

    let conn = db.conn.lock().unwrap();
    let backup_info = repository::record_backup(
        &conn,
        &file_name,
        &target_path.to_string_lossy(),
        file_size_bytes,
        notes.as_deref(),
    )
    .map_err(|e| e.to_string())?;

    // Auto-prune old backups (keep latest 15)
    if let Ok(all_backups) = repository::list_backups(&conn) {
        if all_backups.len() > 15 {
            for old in &all_backups[15..] {
                let _ = fs::remove_file(&old.file_path);
                let _ = repository::delete_backup(&conn, old.id);
            }
        }
    }

    Ok(backup_info)
}

#[tauri::command]
pub fn list_backups(db: State<'_, DbState>) -> AppResult<Vec<BackupInfo>> {
    let conn = db.conn.lock().unwrap();
    repository::list_backups(&conn)
}

#[tauri::command]
pub fn delete_backup(id: i64, db: State<'_, DbState>) -> Result<(), String> {
    let conn = db.conn.lock().unwrap();
    let file_path = repository::delete_backup(&conn, id).map_err(|e| e.to_string())?;
    if let Some(p) = file_path {
        let _ = fs::remove_file(PathBuf::from(p));
    }
    Ok(())
}

/// Restores the database from a specified backup snapshot using SQLite Online Backup API.
/// Automatically creates a pre-restore backup first to guarantee zero accidental data loss.
/// Automatically creates a pre-restore backup first to guarantee zero accidental data loss.
#[tauri::command]
pub fn restore_database_backup(backup_id: i64, db: State<'_, DbState>) -> Result<String, String> {
    let (backup_file_path, backup_name) = {
        let conn = db.conn.lock().unwrap();
        let backup = repository::get_backup_by_id(&conn, backup_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("未找到 ID 为 {} 的备份记录", backup_id))?;
        (PathBuf::from(backup.file_path), backup.file_name)
    };

    if !backup_file_path.exists() {
        return Err(format!(
            "备份文件已不存在于本地磁盘: {:?}",
            backup_file_path
        ));
    }

    // 0. Pre-validate the source backup file integrity and structure before modifying active DB
    {
        let src_test_conn = rusqlite::Connection::open_with_flags(
            &backup_file_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(|e| format!("无法打开源备份文件: {}", e))?;

        let test_check: Result<String, _> =
            src_test_conn.query_row("PRAGMA integrity_check", [], |r| r.get(0));
        match test_check {
            Ok(ref msg) if msg == "ok" => {}
            Ok(msg) => {
                return Err(format!(
                    "源备份文件完整性检查异常 (已拒绝恢复以保护现有数据): {}",
                    msg
                ))
            }
            Err(e) => return Err(format!("无法读取源备份文件完整性信息: {}", e)),
        }

        let has_urls: Result<i64, _> = src_test_conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='urls'",
            [],
            |r| r.get(0),
        );
        if has_urls.unwrap_or(0) == 0 {
            return Err(
                "源备份文件格式不兼容：缺少核心 urls 数据表，非有效历史备份文件".to_string(),
            );
        }
    }

    let backups_dir = db.app_dir.join("backups");
    let _ = fs::create_dir_all(&backups_dir);

    // 1. Safety Pre-restore Backup of the current active DB
    let pre_restore_name = format!(
        "archive_pre_restore_{}.db",
        Utc::now().format("%Y%m%d_%H%M%S")
    );
    let pre_restore_path = backups_dir.join(&pre_restore_name);

    let mut conn = db.conn.lock().unwrap();

    // Safely snapshot current active state via Online Backup API
    {
        let mut pre_dest_conn = rusqlite::Connection::open(&pre_restore_path)
            .map_err(|e| format!("无法创建恢复前快照目标: {}", e))?;
        let pre_backup = rusqlite::backup::Backup::new(&conn, &mut pre_dest_conn)
            .map_err(|e| format!("初始化恢复前快照失败: {}", e))?;
        pre_backup
            .run_to_completion(100, std::time::Duration::from_millis(5), None)
            .map_err(|e| format!("执行恢复前快照失败: {}", e))?;
    }

    let pre_size = fs::metadata(&pre_restore_path)
        .map(|m| m.len())
        .unwrap_or(0);

    // 2. Perform online restore into active connection with rollback guarantee
    let restore_result = (|| -> Result<(), String> {
        let src_conn = rusqlite::Connection::open_with_flags(
            &backup_file_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(|e| format!("无法打开源备份文件: {}", e))?;

        {
            let restore_op = rusqlite::backup::Backup::new(&src_conn, &mut conn)
                .map_err(|e| format!("初始化 SQLite 在线还原失败: {}", e))?;
            restore_op
                .run_to_completion(200, std::time::Duration::from_millis(5), None)
                .map_err(|e| format!("执行在线还原数据流失败: {}", e))?;
        }

        // 3. Flush WAL & validate integrity on restored conn
        let _: Result<i32, _> = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0));
        let integrity_check: Result<String, _> =
            conn.query_row("PRAGMA integrity_check", [], |r| r.get(0));
        match integrity_check {
            Ok(ref msg) if msg == "ok" => {}
            Ok(msg) => return Err(format!("还原后的数据库完整性异常: {}", msg)),
            Err(e) => return Err(format!("还原后的数据库完整性检查失败: {}", e)),
        }

        // 4. Run schema migrations in case backup was from an older version
        run_migrations(&mut conn).map_err(|e| format!("恢复后数据库架构迁移执行失败: {}", e))?;

        Ok(())
    })();

    if let Err(err_msg) = restore_result {
        error!(
            "Database restore failed: {}. Rolling back to pre-restore snapshot...",
            err_msg
        );
        let rollback_res: Result<(), String> = (|| {
            let pre_conn = rusqlite::Connection::open_with_flags(
                &pre_restore_path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .map_err(|e| format!("无法打开安全快照: {}", e))?;

            {
                let rollback_op = rusqlite::backup::Backup::new(&pre_conn, &mut conn)
                    .map_err(|e| format!("初始化回滚复制失败: {}", e))?;
                rollback_op
                    .run_to_completion(200, std::time::Duration::from_millis(5), None)
                    .map_err(|e| format!("执行回滚复制失败: {}", e))?;
            }

            let _: Result<i32, _> =
                conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0));
            let check: String = conn
                .query_row("PRAGMA quick_check", [], |r| r.get(0))
                .map_err(|e| format!("回滚后完整性检查执行失败: {}", e))?;
            if check != "ok" {
                return Err(format!("回滚后数据库完整性校验异常: {}", check));
            }
            Ok(())
        })();

        return match rollback_res {
            Ok(()) => Err(format!(
                "数据库恢复失败并已自动回滚至恢复前状态: {}",
                err_msg
            )),
            Err(rollback_err) => {
                error!(
                    "CRITICAL: Restore failed ({}) AND rollback failed ({})",
                    err_msg, rollback_err
                );
                Err(format!(
                    "严重故障：数据库恢复失败 ({})，且自动回滚执行失败 ({})。请手动检查安全快照: {}",
                    err_msg,
                    rollback_err,
                    pre_restore_path.display()
                ))
            }
        };
    }

    // 5. Persist pre-restore snapshot record into the newly restored database so it is visible in UI.
    // The snapshot file is already durable; report a visible warning if its metadata cannot be saved.
    let snapshot_note = match repository::record_backup(
        &conn,
        &pre_restore_name,
        &pre_restore_path.to_string_lossy(),
        pre_size,
        Some("恢复操作前系统自动快照"),
    ) {
        Ok(_) => format!("恢复前系统已自动保存快照: {}", pre_restore_name),
        Err(e) => {
            error!(
                "Database restored, but failed to persist pre-restore snapshot record '{}': {}",
                pre_restore_name, e
            );
            format!(
                "恢复前快照文件已创建但未登记到备份列表（{}），文件路径: {}",
                e,
                pre_restore_path.display()
            )
        }
    };

    info!(
        "Database restored successfully from backup '{}'. Pre-restore snapshot created at '{}'",
        backup_name, pre_restore_name
    );

    Ok(format!(
        "已成功从备份 '{}' 恢复数据库！{}",
        backup_name, snapshot_note
    ))
}

/// Exports a database backup to a user-chosen target file location
#[tauri::command]
pub fn export_database_backup(
    backup_id: i64,
    target_file_path: String,
    db: State<'_, DbState>,
) -> Result<(), String> {
    let source_path = {
        let conn = db.conn.lock().unwrap();
        let backup = repository::get_backup_by_id(&conn, backup_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("未找到 ID 为 {} 的备份记录", backup_id))?;
        PathBuf::from(backup.file_path)
    };

    if !source_path.exists() {
        return Err("原备份文件不存在".to_string());
    }

    let target = PathBuf::from(target_file_path);
    if let Some(parent) = target.parent() {
        let _ = fs::create_dir_all(parent);
    }

    fs::copy(&source_path, &target).map_err(|e| format!("导出备份文件失败: {}", e))?;
    Ok(())
}

#[tauri::command]
pub fn check_database_integrity(db: State<'_, DbState>) -> AppResult<IntegrityReport> {
    let conn = db.conn.lock().unwrap();
    repository::check_database_integrity(&conn)
}

#[tauri::command]
pub fn vacuum_database(db: State<'_, DbState>) -> AppResult<()> {
    let conn = db.conn.lock().unwrap();
    repository::vacuum_database(&conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exponential_lockout() {
        let (lock1, _) = calculate_exponential_lockout(3);
        assert_eq!(lock1, 0);

        let (lock5, _) = calculate_exponential_lockout(5);
        assert!(lock5 > 0);

        let (lock10, _) = calculate_exponential_lockout(10);
        assert!(lock10 > lock5);

        let (lock15, _) = calculate_exponential_lockout(15);
        assert!(lock15 > lock10);
    }
}
