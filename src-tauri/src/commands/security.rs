use crate::database::models::{BackupInfo, IntegrityReport, PrivacyRule, SecurityState};
use crate::database::repository;
use crate::database::DbState;
use crate::error::AppResult;
use crate::security::master_key::{
    create_master_key_envelope, rewrap_master_key_envelope, unwrap_master_key_with_pin,
    unwrap_master_key_with_recovery, MasterKeyEnvelope,
};
use crate::security::vault::VaultStatus;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tauri::State;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultStatusInfo {
    pub status: VaultStatus,
    pub has_envelope: bool,
    pub has_dpapi_wrap: bool,
    pub has_plaintext_db: bool,
    pub is_portable: bool,
    pub recovery_key_pending: bool,
}

#[tauri::command]
pub async fn get_plaintext_residue(
    db: State<'_, DbState>,
) -> AppResult<crate::security::plaintext::PlaintextResidueInfo> {
    let operation = db.begin_operation()?;
    let app_dir = db.app_dir.clone();
    super::blocking::run(move || {
        operation.check()?;
        let result = crate::security::plaintext::inspect(&app_dir);
        operation.check()?;
        result
    })
    .await?
}

#[tauri::command]
pub async fn retire_legacy_plaintext_backup(
    password: String,
    confirmation: String,
    db: State<'_, DbState>,
) -> Result<String, String> {
    let password = zeroize::Zeroizing::new(password);
    let operation = db.begin_operation().map_err(|error| error.to_string())?;
    let directory = db.app_dir.clone();
    let key = db.vault.master_key().ok_or("保险库主密钥不可用")?;
    let envelope = db.vault.envelope().ok_or("保险库信封不可用")?;
    tauri::async_runtime::spawn_blocking(move || {
        let result = crate::security::plaintext::retire_checked(&directory, &key, &envelope, &password, &confirmation, (|| operation.check(),operation.cancellation_probe()));
        // Retirement has its own final session check before unlink. Once it
        // commits, a later lock must not turn a completed removal into failure.
        result.map(|id| format!("旧明文副本已移除，加密恢复配对保存在 plaintext-retired/{id}/。这不代表磁盘物理擦除。"))
            .map_err(|error| error.to_string())
    }).await.map_err(|_| "旧副本归档任务未完成，请检查状态后重试".to_string())?
}

// ==========================================
// Database Vault Lifecycle Commands
// ==========================================

/// Post-unlock lifecycle hook:
/// 1. Dynamically mounts active browser directories into the history debouncer
/// 2. Migrates legacy plaintext disk page archives into the encrypted SQLCipher database
/// 3. Initiates periodic automated backup check in background
pub fn post_unlock_lifecycle(db: &DbState) {
    if let Ok(policy) = db.with_conn(repository::get_security_state) {
        crate::security::power::set_lock_on_sleep(policy.lock_on_sleep);
    }
    if db
        .vault
        .envelope()
        .map(|env| !env.recovery_key_confirmed)
        .unwrap_or(true)
    {
        return;
    }
    let state = db.clone();
    let operation = match state.begin_operation() {
        Ok(operation) => operation,
        Err(_) => return,
    };
    std::thread::spawn(move || {
        if operation.check().is_err() {
            return;
        }
        // 1. Refresh intelligent history watcher directories
        crate::history::watcher::refresh_active_directories(&state);

        // 2. Migrate legacy plaintext disk archives (from early versions) into encrypted database
        if crate::archive::get_archive_base_dir(&state.app_dir).exists()
            && operation
                .with_conn(|conn| {
                    crate::archive::migrate_legacy_disk_archives_checked(
                        &state.app_dir,
                        conn,
                        || operation.check(),
                    )
                })
                .is_err()
        {
            tracing::warn!("旧归档尚未全部迁移，已保留源文件，跳过本次自动备份");
            return;
        }

        // 3. Automated backup check (delayed briefly so startup unlock views and initial sync
        // do not contend with heavy backup I/O)
        for _ in 0..50 {
            if operation.check().is_err() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if operation.check().is_ok() {
            check_and_run_auto_backup(&state);
        }

        // 4. Reconcile Windows Task Scheduler path/config and catch up missed/failed auto-sync runs
        if operation.check().is_ok() {
            match crate::auto_sync::runner::reconcile_and_check_due_on_startup(&state, &operation) {
                Ok(true) => {
                    for _ in 0..20 {
                        if operation.check().is_err() {
                            return;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    // Wait briefly if another startup sync (e.g. frontend first-unlock sync)
                    // is currently holding the process-wide coordinator lease.
                    for _ in 0..150 {
                        if operation.check().is_err() {
                            return;
                        }
                        if !crate::history::coordinator::global_sync_coordinator().is_running() {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    if operation.check().is_ok() {
                        match crate::auto_sync::runner::execute_auto_sync_in_app(
                            &state,
                            &operation,
                            crate::database::models::SyncTriggerType::Startup,
                        ) {
                            Ok(run) => {
                                tracing::info!(
                                    run_id = run.id,
                                    inserted = run.total_inserted,
                                    "Startup catch-up auto-sync completed"
                                );
                                crate::history::watcher::emit_app_event(
                                    "browsory://auto-sync-updated",
                                );
                                crate::history::watcher::emit_app_event("browsory://sync-status");
                            }
                            Err(err) => {
                                tracing::warn!(
                                    "Startup catch-up auto-sync skipped/failed: {}",
                                    err
                                );
                            }
                        }
                    }
                }
                Ok(false) => {}
                Err(err) => {
                    tracing::warn!("Auto-sync startup reconciliation failed: {}", err);
                }
            }
        }
    });
}

#[tauri::command]
pub async fn get_vault_status(db: State<'_, DbState>) -> AppResult<VaultStatusInfo> {
    let db = db.inner().clone();
    super::blocking::run(move || {
        let status = db.vault.status();
        let has_envelope = db.vault.envelope_path().exists();
        let has_plaintext_db =
            crate::database::encrypt_migration::is_plain_sqlite_database(&db.vault.db_path());
        let is_portable = crate::database::connection::is_portable_install(
            db.app_dir.parent().unwrap_or(&db.app_dir),
        );
        let envelope = db.vault.envelope();
        Ok(VaultStatusInfo {
            status,
            has_envelope,
            has_dpapi_wrap: envelope
                .as_ref()
                .map(|env| env.has_dpapi_wrap())
                .unwrap_or(false),
            has_plaintext_db,
            is_portable,
            recovery_key_pending: envelope
                .map(|env| !env.recovery_key_confirmed)
                .unwrap_or(false),
        })
    })
    .await?
}

#[tauri::command]
pub async fn create_vault(password: String, db: State<'_, DbState>) -> Result<String, String> {
    let generation = db.vault.request_generation()?;
    let db = db.inner().clone();
    super::blocking::run(move || {
        let rec_key = db.vault.create_vault_at_generation(&password, generation)?;
        post_unlock_lifecycle(&db);
        Ok(rec_key)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn unlock_vault(password: String, db: State<'_, DbState>) -> Result<bool, String> {
    let generation = db.vault.request_generation()?;
    let db = db.inner().clone();
    super::blocking::run(move || {
        db.vault
            .unlock_with_password_at_generation(&password, generation)?;
        #[cfg(target_os = "windows")]
        if db
            .vault
            .envelope()
            .is_some_and(|env| env.recovery_key_confirmed)
        {
            if let Err(err) = db.vault.enroll_dpapi_on_unlocked() {
                tracing::warn!("自动升级旧版口令保险库为本机密钥失败: {}", err);
            }
        }
        post_unlock_lifecycle(&db);
        Ok(true)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn resume_passive_vault(
    pin: Option<String>,
    db: State<'_, DbState>,
) -> Result<crate::security::vault::PassiveResumeResult, String> {
    let generation = db.vault.request_generation()?;
    let db_inner = db.inner().clone();
    super::blocking::run(move || {
        let result = db_inner.vault.resume_passive_vault_at_generation(
            |conn| {
                let now = Utc::now().timestamp_millis();
                let creds =
                    repository::get_security_credentials(conn).map_err(|e| e.to_string())?;
                if !creds.pin_enabled {
                    return Ok(crate::security::vault::PassivePinCheck::AllowUnlock);
                }
                if creds.locked_until > now {
                    let rem = ((creds.locked_until - now + 999) / 1000).max(1);
                    let had_pin = pin
                        .as_deref()
                        .map(str::trim)
                        .is_some_and(|value| !value.is_empty());
                    return Ok(crate::security::vault::PassivePinCheck::LockedOut {
                        remaining_secs: rem,
                        message: had_pin.then(|| format!("安全保护冷却中，请在 {} 秒后重试", rem)),
                    });
                }
                let Some(pin_value) = pin
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    return Ok(crate::security::vault::PassivePinCheck::PinRequired);
                };
                let envelope_opt: Option<String> =
                    repository::get_master_key_envelope(conn).map_err(|error| error.to_string())?;
                if verify_pin_material(&creds, pin_value, envelope_opt.as_deref())? {
                    repository::reset_pin_failures(conn).map_err(|error| error.to_string())?;
                    Ok(crate::security::vault::PassivePinCheck::AllowUnlock)
                } else {
                    let new_fails = creds.failed_attempts + 1;
                    let (new_lock, message) = calculate_exponential_lockout(new_fails);
                    repository::record_pin_failure(conn, new_fails, new_lock)
                        .map_err(|e| e.to_string())?;
                    let lockout_remaining_secs = if new_lock > now {
                        ((new_lock - now + 999) / 1000).max(1)
                    } else {
                        0
                    };
                    Ok(crate::security::vault::PassivePinCheck::InvalidPin {
                        is_locked_out: new_lock > now,
                        lockout_remaining_secs,
                        message,
                    })
                }
            },
            generation,
        )?;
        if result.unlocked {
            post_unlock_lifecycle(&db_inner);
        }
        Ok(result)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn unlock_vault_with_recovery_key(
    recovery_key: String,
    db: State<'_, DbState>,
) -> Result<bool, String> {
    let generation = db.vault.request_generation()?;
    let db = db.inner().clone();
    super::blocking::run(move || {
        db.vault
            .unlock_with_recovery_key_at_generation(&recovery_key, generation)?;
        #[cfg(target_os = "windows")]
        if db
            .vault
            .envelope()
            .is_some_and(|env| env.recovery_key_confirmed)
        {
            if let Err(err) = db.vault.enroll_dpapi_on_unlocked() {
                tracing::warn!("通过恢复密钥解锁后自动绑定本机密钥失败: {}", err);
            }
        }
        post_unlock_lifecycle(&db);
        Ok(true)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn lock_vault(db: State<'_, DbState>) -> Result<(), String> {
    let vault = db.vault.clone();
    vault.revoke_active_session();
    tauri::async_runtime::spawn_blocking(move || vault.lock())
        .await
        .map_err(|error| format!("锁定任务失败: {}", error))?
}

#[tauri::command]
pub async fn change_vault_password(
    old_password: String,
    new_password: String,
    db: State<'_, DbState>,
) -> Result<(), String> {
    let db = db.inner().clone();
    super::blocking::run(move || db.vault.change_password(&old_password, &new_password))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn regenerate_vault_recovery_key(
    password: String,
    db: State<'_, DbState>,
) -> Result<String, String> {
    let db = db.inner().clone();
    super::blocking::run(move || db.vault.regenerate_unconfirmed_recovery_key(&password))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn regenerate_passive_vault_recovery_key(
    db: State<'_, DbState>,
) -> Result<String, String> {
    let db = db.inner().clone();
    super::blocking::run(move || db.vault.regenerate_unconfirmed_recovery_key_from_session())
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn confirm_vault_recovery_key(
    recovery_key: String,
    db: State<'_, DbState>,
) -> Result<(), String> {
    let db = db.inner().clone();
    super::blocking::run(move || {
        if db.vault.confirm_recovery_key(&recovery_key)? {
            #[cfg(target_os = "windows")]
            if let Err(err) = db.vault.enroll_dpapi_on_unlocked() {
                tracing::warn!("确认恢复密钥后自动绑定本机密钥失败: {}", err);
            }
            post_unlock_lifecycle(&db);
        }
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn migrate_legacy_database(
    password: String,
    db: State<'_, DbState>,
) -> Result<String, String> {
    let generation = db.vault.request_generation()?;
    let db = db.inner().clone();
    super::blocking::run(move || {
        let rec_key = db
            .vault
            .migrate_legacy_plaintext_at_generation(&password, generation)?;
        post_unlock_lifecycle(&db);
        Ok(rec_key)
    })
    .await
    .map_err(|error| error.to_string())?
}

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

fn ensure_pin_not_in_cooldown(locked_until: i64, now: i64) -> Result<(), String> {
    if locked_until > now {
        let rem = ((locked_until - now + 999) / 1000).max(1);
        Err(format!("安全保护冷却中，请在 {} 秒后重试", rem))
    } else {
        Ok(())
    }
}

fn verify_pin_material(
    creds: &repository::SecurityCredentials,
    pin: &str,
    envelope_json: Option<&str>,
) -> Result<bool, String> {
    if let Some(env_json) = envelope_json {
        let envelope: MasterKeyEnvelope =
            serde_json::from_str(env_json).map_err(|e| format!("解析主密钥信封失败: {}", e))?;
        Ok(unwrap_master_key_with_pin(&envelope, pin).is_ok())
    } else {
        let salt = creds.pin_salt.as_deref().unwrap_or_default();
        let expected = creds.pin_hash.as_deref().unwrap_or_default();
        Ok(!expected.is_empty() && hash_pin_with_salt(pin, salt) == expected)
    }
}

fn record_failed_pin_attempt(
    conn: &rusqlite::Connection,
    creds: &repository::SecurityCredentials,
) -> Result<String, String> {
    let new_fails = creds.failed_attempts + 1;
    let (new_lock, msg) = calculate_exponential_lockout(new_fails);
    repository::record_pin_failure(conn, new_fails, new_lock).map_err(|e| e.to_string())?;
    Ok(msg)
}

#[tauri::command]
pub async fn get_security_state(db: State<'_, DbState>) -> AppResult<SecurityState> {
    let operation = db.begin_operation()?;
    super::blocking::run(move || {
        operation.check()?;
        let result = operation.with_read_conn(repository::get_security_state);
        operation.check()?;
        result
    })
    .await?
}

#[tauri::command]
pub async fn verify_pin(pin: String, db: State<'_, DbState>) -> Result<bool, String> {
    let operation = db.begin_operation().map_err(|e| e.to_string())?;
    super::blocking::run(move || {
        operation.check().map_err(|error| error.to_string())?;

        let result = (|| {
            let now = Utc::now().timestamp_millis();
            let conn_arc = operation.connection().map_err(|e| e.to_string())?;
            let conn = conn_arc.lock().map_err(|e| e.to_string())?;

            let creds = repository::get_security_credentials(&conn).map_err(|e| e.to_string())?;

            if !creds.pin_enabled {
                return Ok(true);
            }

            ensure_pin_not_in_cooldown(creds.locked_until, now)?;

            // 1. Check if Argon2id Master Key Envelope exists
            // A metadata read failure must not silently downgrade verification
            // to the legacy hash path. That could accept stale credentials
            // while the current envelope is unavailable.
            let envelope_opt: Option<String> =
                repository::get_master_key_envelope(&conn).map_err(|error| error.to_string())?;

            // Verification is deliberately read-only. Legacy material is
            // migrated only by the explicit PIN change flow, which can return
            // the newly generated recovery key to the UI.
            let is_valid = verify_pin_material(&creds, &pin, envelope_opt.as_deref())?;

            if is_valid {
                repository::reset_pin_failures(&conn).map_err(|error| error.to_string())?;
                Ok(true)
            } else {
                let msg = record_failed_pin_attempt(&conn, &creds)?;
                Err(msg)
            }
        })();
        operation.check().map_err(|e| e.to_string())?;
        result
    })
    .await
    .map_err(|error| error.to_string())?
}

fn validate_four_digit_pin(raw: &str) -> Result<&str, String> {
    let trimmed = raw.trim();
    if trimmed.len() != 4 || !trimmed.chars().all(|c| c.is_ascii_digit()) {
        return Err("PIN 码必须为 4 位纯数字".to_string());
    }
    Ok(trimmed)
}

#[tauri::command]
pub async fn set_or_change_pin(
    old_pin: Option<String>,
    new_pin: String,
    db: State<'_, DbState>,
) -> Result<Option<String>, String> {
    let operation = db.begin_operation().map_err(|e| e.to_string())?;
    super::blocking::run(move || {
        operation.check().map_err(|error| error.to_string())?;

        let result = (|| {
            let trimmed_new = validate_four_digit_pin(&new_pin)?;

            let conn_arc = operation.connection().map_err(|e| e.to_string())?;
            let conn = conn_arc.lock().map_err(|e| e.to_string())?;
            let creds = repository::get_security_credentials(&conn).map_err(|e| e.to_string())?;

            let now = Utc::now().timestamp_millis();
            ensure_pin_not_in_cooldown(creds.locked_until, now)?;
            let existing_envelope_json =
                repository::get_master_key_envelope(&conn).map_err(|error| error.to_string())?;

            // Validate the old PIN before opening the replacement transaction.
            // A failed attempt must survive any later business transaction
            // rollback, otherwise repeated PIN changes could bypass lockout.
            if creds.pin_enabled {
                let old = old_pin
                    .as_deref()
                    .ok_or_else(|| "请输入原 PIN 码以确认身份".to_string())?;
                let is_valid = verify_pin_material(&creds, old, existing_envelope_json.as_deref())?;
                if !is_valid {
                    let msg = record_failed_pin_attempt(&conn, &creds)?;
                    return Err(msg);
                }
            }

            // Envelope replacement and app_security update are one logical
            // state transition. Keep the old envelope/PIN usable if any step
            // fails, including cancellation before commit.
            let transaction = conn
                .unchecked_transaction()
                .map_err(|error| format!("开启 PIN 更新事务失败: {}", error))?;
            let transaction_result: Result<Option<String>, String> = (|| {
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

                        repository::save_master_key_envelope(&transaction, &updated_json, None)
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
                        repository::save_master_key_envelope(&transaction, &env_json, None)
                            .map_err(|e| e.to_string())?;
                        return_recovery_key = Some(rec_key);
                    }
                } else {
                    // Setting PIN for the first time: generate Master Key and Recovery Key
                    let (envelope, rec_key, _) = create_master_key_envelope(trimmed_new, now)?;
                    let env_json = serde_json::to_string(&envelope)
                        .map_err(|e| format!("序列化主密钥信封失败: {}", e))?;
                    repository::save_master_key_envelope(&transaction, &env_json, None)
                        .map_err(|e| e.to_string())?;
                    return_recovery_key = Some(rec_key);
                }

                // Set pin_enabled = 1, but NEVER store weak SHA-256 hash
                repository::update_pin(&transaction, None, None).map_err(|e| e.to_string())?;
                operation.check().map_err(|error| error.to_string())?;
                Ok(return_recovery_key)
            })();

            match transaction_result {
                Ok(recovery_key) => transaction
                    .commit()
                    .map(|()| recovery_key)
                    .map_err(|error| format!("提交 PIN 更新事务失败: {}", error)),
                Err(error) => Err(error),
            }
        })();
        // The cancellation check is part of the transaction before COMMIT.
        // A later revocation must not report failure after the new PIN has
        // already been durably committed.
        result
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn verify_recovery_key(
    recovery_key: String,
    db: State<'_, DbState>,
) -> Result<bool, String> {
    let operation = db.begin_operation().map_err(|e| e.to_string())?;
    super::blocking::run(move || {
        operation.check().map_err(|error| error.to_string())?;

        let result = (|| {
            let conn_arc = operation.connection().map_err(|e| e.to_string())?;
            let conn = conn_arc.lock().map_err(|e| e.to_string())?;
            let envelope_json = repository::get_master_key_envelope(&conn)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "当前系统未初始化主密钥信封".to_string())?;

            let envelope: MasterKeyEnvelope = serde_json::from_str(&envelope_json)
                .map_err(|e| format!("解析主密钥信封失败: {}", e))?;

            unwrap_master_key_with_recovery(&envelope, &recovery_key).map(|_| true)
        })();
        operation.check().map_err(|e| e.to_string())?;
        result
    })
    .await
    .map_err(|error| error.to_string())?
}

fn reset_pin_with_recovery_on_conn(
    conn: &rusqlite::Connection,
    recovery_key: &str,
    trimmed_new: &str,
    check: impl Fn() -> Result<(), String>,
) -> Result<(), String> {
    let envelope_json = repository::get_master_key_envelope(conn)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "当前系统未初始化主密钥信封".to_string())?;

    let mut envelope: MasterKeyEnvelope =
        serde_json::from_str(&envelope_json).map_err(|e| format!("解析主密钥信封失败: {}", e))?;

    let master_key = unwrap_master_key_with_recovery(&envelope, recovery_key)
        .map_err(|_| "安全恢复密钥校验失败，无法重置 PIN 码".to_string())?;

    let now = Utc::now().timestamp_millis();
    rewrap_master_key_envelope(&mut envelope, &master_key, trimmed_new, now)?;

    let updated_json =
        serde_json::to_string(&envelope).map_err(|e| format!("序列化主密钥信封失败: {}", e))?;

    let transaction = conn
        .unchecked_transaction()
        .map_err(|error| format!("开启 PIN 重置事务失败: {}", error))?;
    repository::save_master_key_envelope(&transaction, &updated_json, None)
        .map_err(|e| e.to_string())?;
    // Update security table: PIN enabled, but no weak SHA-256 hash stored
    repository::update_pin(&transaction, None, None).map_err(|e| e.to_string())?;
    repository::reset_pin_failures(&transaction).map_err(|e| e.to_string())?;
    check()?;
    transaction
        .commit()
        .map_err(|error| format!("提交 PIN 重置事务失败: {}", error))?;

    Ok(())
}

#[tauri::command]
pub async fn reset_pin_with_recovery_key(
    recovery_key: String,
    new_pin: String,
    db: State<'_, DbState>,
) -> Result<(), String> {
    match db.begin_operation() {
        Ok(operation) => super::blocking::run(move || {
            operation.check().map_err(|error| error.to_string())?;

            let trimmed_new = validate_four_digit_pin(&new_pin)?;
            let conn_arc = operation.connection().map_err(|e| e.to_string())?;
            let conn = conn_arc.lock().map_err(|e| e.to_string())?;
            reset_pin_with_recovery_on_conn(&conn, &recovery_key, trimmed_new, || {
                operation.check().map_err(|error| error.to_string())
            })
        })
        .await
        .map_err(|error| error.to_string())?,
        Err(lock_err) => {
            let can_passive_resume = db.vault.status() == VaultStatus::Locked
                && db.vault.envelope().is_some_and(|env| env.has_dpapi_wrap());
            if !can_passive_resume {
                return Err(lock_err.to_string());
            }
            let generation = db.vault.request_generation()?;
            let db_inner = db.inner().clone();
            super::blocking::run(move || {
                let trimmed_new = validate_four_digit_pin(&new_pin)?;
                let result = db_inner.vault.resume_passive_vault_at_generation(
                    |conn| {
                        reset_pin_with_recovery_on_conn(conn, &recovery_key, trimmed_new, || {
                            db_inner.vault.check_generation(generation)
                        })?;
                        Ok(crate::security::vault::PassivePinCheck::AllowUnlock)
                    },
                    generation,
                )?;
                if result.unlocked {
                    post_unlock_lifecycle(&db_inner);
                }
                Ok(())
            })
            .await
            .map_err(|error| error.to_string())?
        }
    }
}

#[tauri::command]
pub async fn disable_pin(current_pin: String, db: State<'_, DbState>) -> Result<(), String> {
    let operation = db.begin_operation().map_err(|e| e.to_string())?;
    super::blocking::run(move || {
        operation.check().map_err(|error| error.to_string())?;

        let result = (|| {
            let conn_arc = operation.connection().map_err(|e| e.to_string())?;
            let conn = conn_arc.lock().map_err(|e| e.to_string())?;
            let creds = repository::get_security_credentials(&conn).map_err(|e| e.to_string())?;

            if !creds.pin_enabled {
                return Ok(());
            }

            let now = Utc::now().timestamp_millis();
            ensure_pin_not_in_cooldown(creds.locked_until, now)?;

            let envelope_json =
                repository::get_master_key_envelope(&conn).map_err(|e| e.to_string())?;

            if let Some(env_json) = envelope_json {
                if !verify_pin_material(&creds, &current_pin, Some(&env_json))? {
                    let msg = record_failed_pin_attempt(&conn, &creds)?;
                    return Err(msg);
                }
            } else if !verify_pin_material(&creds, &current_pin, None)? {
                let msg = record_failed_pin_attempt(&conn, &creds)?;
                return Err(msg);
            }

            let transaction = conn
                .unchecked_transaction()
                .map_err(|error| format!("开启停用 PIN 事务失败: {}", error))?;
            repository::clear_pin(&transaction).map_err(|e| e.to_string())?;
            repository::clear_master_key_envelope(&transaction).map_err(|e| e.to_string())?;
            operation.check().map_err(|error| error.to_string())?;
            transaction
                .commit()
                .map_err(|error| format!("提交停用 PIN 事务失败: {}", error))?;
            Ok(())
        })();
        result
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn update_security_options(
    auto_lock_minutes: i64,
    lock_on_minimize: bool,
    lock_on_sleep: bool,
    db: State<'_, DbState>,
) -> AppResult<()> {
    let operation = db.begin_operation()?;
    super::blocking::run(move || {
        operation.check()?;

        let result = (|| {
            let conn_arc = operation.connection()?;
            let conn = conn_arc.lock()?;
            repository::update_security_options(
                &conn,
                auto_lock_minutes,
                lock_on_minimize,
                lock_on_sleep,
            )
        })();
        operation.check()?;
        if result.is_ok() {
            crate::security::power::set_lock_on_sleep(lock_on_sleep);
        }
        result
    })
    .await?
}

// ==========================================
// Privacy Rules Commands
// ==========================================

#[tauri::command]
pub async fn list_privacy_rules(db: State<'_, DbState>) -> AppResult<Vec<PrivacyRule>> {
    let operation = db.begin_operation()?;
    super::blocking::run(move || {
        operation.check()?;
        let result = operation.with_read_conn(repository::list_privacy_rules);
        operation.check()?;
        result
    })
    .await?
}

#[tauri::command]
pub async fn add_privacy_rule(
    pattern: String,
    rule_type: String,
    db: State<'_, DbState>,
) -> Result<PrivacyRule, String> {
    let operation = db.begin_operation().map_err(|e| e.to_string())?;
    super::blocking::run(move || {
        operation.check().map_err(|error| error.to_string())?;

        let result = (|| {
            let trimmed = pattern.trim();
            if trimmed.is_empty() {
                return Err("规则匹配模式不能为空".to_string());
            }
            let normalized_type = rule_type.trim().to_lowercase();
            if !matches!(normalized_type.as_str(), "private" | "hidden") {
                return Err("规则类型必须是 private 或 hidden".to_string());
            }
            let conn_arc = operation.connection().map_err(|e| e.to_string())?;
            let conn = conn_arc.lock().map_err(|e| e.to_string())?;
            repository::add_privacy_rule(&conn, trimmed, &normalized_type)
                .map_err(|e| e.to_string())
        })();
        operation.check().map_err(|e| e.to_string())?;
        result
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn toggle_privacy_rule(id: i64, enabled: bool, db: State<'_, DbState>) -> AppResult<()> {
    let operation = db.begin_operation()?;
    super::blocking::run(move || {
        operation.check()?;

        let result = (|| {
            let conn_arc = operation.connection()?;
            let conn = conn_arc.lock()?;
            repository::toggle_privacy_rule(&conn, id, enabled)
        })();
        operation.check()?;
        result
    })
    .await?
}

#[tauri::command]
pub async fn delete_privacy_rule(id: i64, db: State<'_, DbState>) -> AppResult<()> {
    let operation = db.begin_operation()?;
    super::blocking::run(move || {
        operation.check()?;

        let result = (|| {
            let conn_arc = operation.connection()?;
            let conn = conn_arc.lock()?;
            repository::delete_privacy_rule(&conn, id)
        })();
        operation.check()?;
        result
    })
    .await?
}

// ==========================================
// Database Maintenance, Backup & Restore Commands
// ==========================================

pub fn execute_backup_internal(
    conn: &rusqlite::Connection,
    db: &DbState,
    backups_dir: &std::path::Path,
    prefix: &str,
    notes: Option<&str>,
) -> Result<BackupInfo, String> {
    execute_backup_internal_checked(conn, db, backups_dir, prefix, notes, || Ok(()))
}

pub fn execute_backup_internal_checked<F>(
    conn: &rusqlite::Connection,
    db: &DbState,
    backups_dir: &std::path::Path,
    prefix: &str,
    notes: Option<&str>,
    check: F,
) -> Result<BackupInfo, String>
where
    F: Fn() -> AppResult<()>,
{
    crate::archive::migrate_legacy_disk_archives_checked(&db.app_dir, conn, &check).map_err(
        |e| {
            if matches!(e, crate::error::AppError::VaultLocked(_)) {
                e.to_string()
            } else {
                "旧归档尚未全部通过迁移校验，备份已中止并保留源文件".to_string()
            }
        },
    )?;
    let key = db.vault.master_key().ok_or("保险库主密钥不可用")?;
    let envelope = db.vault.envelope().ok_or("保险库信封不可用")?;
    let backup = crate::security::backup::execute_encrypted_backup_checked(
        conn,
        crate::security::backup::BackupVaultContext {
            app_dir: &db.app_dir,
            backups_dir,
            key: &key,
            envelope: &envelope,
        },
        prefix,
        notes,
        || check().map_err(|e| e.to_string()),
    )?;

    // Reconcile any previously orphaned backup packages so they participate in
    // listing and the 15-backup retention budget.
    if let Err(error) =
        crate::security::backup::backup_maintenance::reconcile_orphan_backups(conn, backups_dir)
    {
        tracing::warn!("Orphan backup reconciliation reported issues: {}", error);
    }

    // Rotation runs only after the package has passed integrity checks,
    // published atomically, and been recorded in the database. A failed file
    // deletion is retained as a record and retried by the next backup.
    if let Err(error) =
        crate::security::backup::backup_maintenance::rotate_archive_backups(conn, backups_dir)
    {
        tracing::warn!("Backup retention is pending and will be retried: {}", error);
    }
    Ok(backup)
}

pub fn execute_backup_with_operation(
    operation: &crate::security::session::VaultOperation,
    db: &DbState,
    backups_dir: &std::path::Path,
    prefix: &str,
    notes: Option<&str>,
) -> Result<BackupInfo, String> {
    operation.check().map_err(|e| e.to_string())?;
    if crate::archive::get_archive_base_dir(&db.app_dir).exists() {
        operation
            .with_conn(|conn| {
                crate::archive::migrate_legacy_disk_archives_checked(&db.app_dir, conn, || {
                    operation.check()
                })
            })
            .map_err(|e| {
                if matches!(e, crate::error::AppError::VaultLocked(_)) {
                    e.to_string()
                } else {
                    "旧归档尚未全部通过迁移校验，备份已中止并保留源文件".to_string()
                }
            })?;
    }
    let key = db.vault.master_key().ok_or("保险库主密钥不可用")?;
    let envelope = db.vault.envelope().ok_or("保险库信封不可用")?;
    let backup = crate::security::backup::execute_encrypted_backup_with_operation(
        operation,
        crate::security::backup::BackupVaultContext {
            app_dir: &db.app_dir,
            backups_dir,
            key: &key,
            envelope: &envelope,
        },
        prefix,
        notes,
    )?;

    if let Err(error) = operation.with_conn(|conn| {
        if let Err(error) =
            crate::security::backup::backup_maintenance::reconcile_orphan_backups(conn, backups_dir)
        {
            tracing::warn!("Orphan backup reconciliation reported issues: {}", error);
        }
        if let Err(error) =
            crate::security::backup::backup_maintenance::rotate_archive_backups(conn, backups_dir)
        {
            tracing::warn!("Backup retention is pending and will be retried: {}", error);
        }
        Ok(())
    }) {
        tracing::warn!("Backup post-maintenance deferred: {}", error);
    }

    Ok(backup)
}

/// Periodic automated background backup check
pub fn check_and_run_auto_backup(db_state: &DbState) {
    if !db_state.vault.is_unlocked() {
        return;
    }
    let backups_dir = db_state.app_dir.join("backups");
    let operation = match db_state.begin_operation() {
        Ok(operation) => operation,
        Err(_) => return,
    };

    let should_run = match operation.with_read_conn(|conn| {
        let auto_backup_enabled = repository::get_setting(conn, "auto_backup_enabled")
            .unwrap_or(None)
            .unwrap_or_else(|| "1".to_string())
            == "1";

        if !auto_backup_enabled {
            return Ok(false);
        }

        let interval_days: i64 = repository::get_setting(conn, "auto_backup_interval_days")
            .unwrap_or(None)
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(1)
            .clamp(1, 365);

        let now_ms = Utc::now().timestamp_millis();
        let interval_ms = interval_days.saturating_mul(24 * 60 * 60 * 1000);

        let last_backup_time: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(created_at), 0) FROM backups WHERE file_name LIKE 'archive_backup_%'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);

        Ok(now_ms - last_backup_time >= interval_ms)
    }) {
        Ok(due) => due,
        Err(_) => return,
    };

    if should_run {
        tracing::info!("Triggering automated periodic database backup...");
        match execute_backup_with_operation(
            &operation,
            db_state,
            &backups_dir,
            "archive_backup_auto",
            Some("系统自动日常备份"),
        ) {
            Ok(b) => tracing::info!("Automated backup created successfully: {}", b.file_name),
            Err(e) => tracing::warn!("Automated backup failed: {}", e),
        }
    }
}

#[tauri::command]
pub async fn create_database_backup(
    notes: Option<String>,
    db: State<'_, DbState>,
) -> Result<BackupInfo, String> {
    let operation = db.begin_operation().map_err(|e| e.to_string())?;
    let db = db.inner().clone();
    super::blocking::run(move || {
        operation.check().map_err(|error| error.to_string())?;

        let backups_dir = db.app_dir.join("backups");
        let result = execute_backup_with_operation(
            &operation,
            &db,
            &backups_dir,
            "archive_backup",
            notes.as_deref(),
        );
        if result.is_err() {
            operation.check().map_err(|e| e.to_string())?;
        }
        result
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn list_backups(db: State<'_, DbState>) -> AppResult<Vec<BackupInfo>> {
    let operation = db.begin_operation()?;
    let db = db.inner().clone();
    super::blocking::run(move || {
        operation.check()?;

        let result = (|| {
            let conn_arc = operation.connection()?;
            let conn = conn_arc.lock()?;
            let backups_dir = db.app_dir.join("backups");
            if let Err(error) =
                crate::security::backup::backup_maintenance::reconcile_orphan_backups(
                    &conn,
                    &backups_dir,
                )
            {
                tracing::warn!("Orphan backup reconciliation reported issues: {}", error);
            }
            repository::list_backups(&conn)
        })();
        operation.check()?;
        result
    })
    .await?
}

#[tauri::command]
pub async fn delete_backup(id: i64, db: State<'_, DbState>) -> Result<(), String> {
    let operation = db.begin_operation().map_err(|e| e.to_string())?;
    let db = db.inner().clone();
    super::blocking::run(move || {
        operation.check().map_err(|error| error.to_string())?;

        let result = (|| {
            let conn_arc = operation.connection().map_err(|e| e.to_string())?;
            let conn = conn_arc.lock().map_err(|e| e.to_string())?;
            let backups_dir = db.app_dir.join("backups");
            crate::security::backup::backup_maintenance::delete_backup_record(
                &conn,
                id,
                &backups_dir,
            )
        })();
        operation.check().map_err(|e| e.to_string())?;
        result
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Restores the database from a specified backup snapshot.
/// Guarantees safe validation, envelope credentials check, and pre-restore backup.
#[tauri::command]
pub async fn restore_database_backup(
    backup_id: i64,
    password: Option<String>,
    recovery_key: Option<String>,
    db: State<'_, DbState>,
) -> Result<String, String> {
    let operation = db.begin_operation().map_err(|e| e.to_string())?;
    let db = db.inner().clone();
    super::blocking::run(move || {
        operation.check().map_err(|error| error.to_string())?;

        let (backup_file_path, _backup_name) = {
            let conn_arc = operation.connection().map_err(|e| e.to_string())?;
            let conn = conn_arc.lock().map_err(|e| e.to_string())?;
            let backup = repository::get_backup_by_id(&conn, backup_id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("未找到 ID 为 {} 的备份记录", backup_id))?;
            (PathBuf::from(backup.file_path), backup.file_name)
        };

        restore_from_path(&backup_file_path, password, recovery_key, &db, operation)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn restore_database_backup_file(
    file_path: String,
    password: Option<String>,
    recovery_key: Option<String>,
    db: State<'_, DbState>,
) -> Result<String, String> {
    let operation = db.begin_operation().map_err(|error| error.to_string())?;
    let db = db.inner().clone();
    super::blocking::run(move || {
        operation.check().map_err(|error| error.to_string())?;

        restore_from_path(
            std::path::Path::new(&file_path),
            password,
            recovery_key,
            &db,
            operation,
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

fn restore_from_path(
    backup_path: &std::path::Path,
    password: Option<String>,
    recovery_key: Option<String>,
    db: &DbState,
    operation: crate::security::session::VaultOperation,
) -> Result<String, String> {
    let password = password.map(zeroize::Zeroizing::new);
    let recovery_key = recovery_key.map(zeroize::Zeroizing::new);
    let mut replacement = None;
    if !backup_path.exists() {
        return Err(format!("备份文件已不存在于本地磁盘: {:?}", backup_path));
    }

    // 1. Prepare master key if session is currently unlocked
    let current_key_opt = db.vault.master_key();
    let current_envelope = db.vault.envelope();

    // 2. Perform safe restore flow:
    // Notice: we only lock the database in `before_replace` callback AFTER all hashes,
    // encryption keys, and database integrity checks pass successfully!
    let (restored_key, restored_env) = crate::security::backup::restore_encrypted_backup_flow(
        backup_path,
        &db.app_dir,
        password.as_ref().map(|value| value.as_str()),
        recovery_key.as_ref().map(|value| value.as_str()),
        current_key_opt.as_ref(),
        current_envelope.as_ref(),
        || {
            // Safe lock hook: only close DB and drop file locks right before atomic replacement
            replacement = Some(
                db.vault
                    .begin_replacement(operation)
                    .map_err(|e| e.to_string())?,
            );
            Ok(())
        },
    )?;

    // Keep the restored vault locked; candidate schema/integrity was already
    // checked before installation. Refresh in-memory metadata under lifecycle.
    let can_auto_resume = restored_env.has_dpapi_wrap()
        && crate::security::envelope::unwrap_master_key_with_dpapi(&restored_env).is_ok();
    drop(restored_key);
    replacement
        .take()
        .ok_or("恢复操作缺少生命周期所有权")?
        .finish_locked(restored_env);
    if can_auto_resume {
        let _ = db.vault.ensure_passive_vault_unlocked();
        if db.vault.is_unlocked() {
            post_unlock_lifecycle(db);
        }
    }
    let resumed = can_auto_resume && db.vault.is_unlocked();
    Ok(if resumed {
        "加密备份已恢复并完成重新加载。".into()
    } else if can_auto_resume {
        "加密备份已恢复，请输入 4 位 PIN 码继续访问。".into()
    } else {
        "加密备份已恢复，保险库保持锁定，请使用备份时的口令或恢复密钥重新解锁。".into()
    })
}

fn copy_backup_export_atomic(
    source: &Path,
    target: &Path,
    mut check: impl FnMut() -> Result<(), String>,
) -> Result<(), String> {
    let source_len = fs::metadata(source)
        .map_err(|error| format!("读取原备份元数据失败: {}", error))?
        .len();
    let parent = target
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|error| format!("创建导出目录失败: {}", error))?;
    let file_name = target
        .file_name()
        .ok_or_else(|| "导出文件名不能为空".to_string())?
        .to_string_lossy();
    let temporary = parent.join(format!(
        ".{}-browsory-export-{}.partial",
        file_name,
        uuid::Uuid::new_v4()
    ));

    let result = (|| {
        check()?;
        let mut input =
            fs::File::open(source).map_err(|error| format!("打开原备份文件失败: {}", error))?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| format!("创建导出临时文件失败: {}", error))?;
        let mut buffer = [0u8; 64 * 1024];
        let mut copied = 0u64;
        loop {
            check()?;
            let read = input
                .read(&mut buffer)
                .map_err(|error| format!("读取原备份文件失败: {}", error))?;
            if read == 0 {
                break;
            }
            output
                .write_all(&buffer[..read])
                .map_err(|error| format!("写入导出临时文件失败: {}", error))?;
            copied = copied
                .checked_add(read as u64)
                .ok_or_else(|| "导出文件大小溢出".to_string())?;
        }
        check()?;
        output
            .flush()
            .map_err(|error| format!("刷新导出临时文件失败: {}", error))?;
        output
            .sync_all()
            .map_err(|error| format!("同步导出临时文件失败: {}", error))?;
        drop(output);

        // The source is a published backup, so a byte-count check catches
        // truncation before the destination is touched. The temp metadata is
        // checked as well to cover filesystems that report short writes late.
        let temporary_len = fs::metadata(&temporary)
            .map_err(|error| format!("校验导出临时文件失败: {}", error))?
            .len();
        if copied != source_len || temporary_len != source_len {
            return Err(format!(
                "导出文件校验失败: 原文件 {} 字节，已复制 {} 字节",
                source_len, copied
            ));
        }
        check()?;

        // Publish in the same directory only after the complete file is
        // durable. Never remove the target first: a failed rename preserves
        // the user's existing export.
        fs::rename(&temporary, target).map_err(|error| format!("发布导出文件失败: {}", error))?;
        #[cfg(unix)]
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| format!("同步导出目录失败: {}", error))?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Exports a database backup to a user-chosen target file location
#[tauri::command]
pub async fn export_database_backup(
    backup_id: i64,
    target_file_path: String,
    db: State<'_, DbState>,
) -> Result<(), String> {
    let operation = db.begin_operation().map_err(|e| e.to_string())?;
    let db = db.inner().clone();
    super::blocking::run(move || {
        operation.check().map_err(|error| error.to_string())?;

        let result = (|| {
            let source_path = {
                let conn_arc = operation.connection().map_err(|e| e.to_string())?;
                let conn = conn_arc.lock().map_err(|e| e.to_string())?;
                let backup = repository::get_backup_by_id(&conn, backup_id)
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| format!("未找到 ID 为 {} 的备份记录", backup_id))?;
                PathBuf::from(backup.file_path)
            };

            if !source_path.exists() {
                return Err("原备份文件不存在".to_string());
            }

            let target = PathBuf::from(target_file_path);
            super::export_destination::validate(&db.app_dir, &target)
                .map_err(|error| error.to_string())?;
            copy_backup_export_atomic(&source_path, &target, || {
                operation.check().map_err(|error| error.to_string())
            })
        })();
        // A cancellation observed after atomic publication must not turn a
        // successful export into a reported failure. Before publication the
        // helper checks cancellation and removes only its temporary file.
        if result.is_err() {
            operation.check().map_err(|e| e.to_string())?;
        }
        result
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn check_database_integrity(db: State<'_, DbState>) -> AppResult<IntegrityReport> {
    let operation = db.begin_operation()?;
    super::blocking::run(move || {
        operation.check()?;

        let result = (|| {
            let conn_arc = operation.connection()?;
            let conn = conn_arc.lock()?;
            repository::check_database_integrity(&conn)
        })();
        operation.check()?;
        result
    })
    .await?
}

#[tauri::command]
pub async fn vacuum_database(db: State<'_, DbState>) -> AppResult<()> {
    let operation = db.begin_operation()?;
    super::blocking::run(move || {
        operation.check()?;

        let result = (|| {
            let conn_arc = operation.connection()?;
            let conn = conn_arc.lock()?;
            repository::vacuum_database(&conn)
        })();
        operation.check()?;
        result
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repository;
    use crate::security::vault::VaultManager;
    use std::sync::Arc;
    use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};

    fn ipc_request(command: &str, body: serde_json::Value) -> tauri::webview::InvokeRequest {
        tauri::webview::InvokeRequest {
            cmd: command.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "http://tauri.localhost".parse().unwrap(),
            body: tauri::ipc::InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.into(),
        }
    }

    fn fixture_state(root: &std::path::Path, vault: Arc<VaultManager>) -> DbState {
        DbState {
            _directory_lock: None,
            vault,
            app_dir: root.to_path_buf(),
            temp_dir: root.join("temp"),
            logs_dir: root.join("logs"),
        }
    }

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

    #[test]
    fn verify_pin_legacy_ipc_keeps_legacy_material_without_silent_migration() {
        let root =
            std::env::temp_dir().join(format!("browsory_pin_verify_ipc_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let vault = Arc::new(VaultManager::unlocked_fixture(root.clone()));
        let salt = "legacy-test-salt";
        let pin = "1234";
        let expected_hash = hash_pin_with_salt(pin, salt);
        vault
            .with_conn(|conn| {
                conn.execute("DELETE FROM master_key_envelope", [])?;
                conn.execute(
                    "UPDATE app_security SET pin_enabled=1, pin_salt=?1, pin_hash=?2, failed_attempts=0, locked_until=0",
                    rusqlite::params![salt, expected_hash],
                )?;
                Ok(())
            })
            .unwrap();

        let state = fixture_state(&root, vault.clone());
        let app = mock_builder()
            .manage(state)
            .invoke_handler(tauri::generate_handler![super::verify_pin])
            .build(mock_context(noop_assets()))
            .unwrap();
        let webview = tauri::WebviewWindowBuilder::new(&app, "verify-pin-ipc", Default::default())
            .build()
            .unwrap();
        let response = get_ipc_response(
            &webview,
            ipc_request("verify_pin", serde_json::json!({"pin": pin})),
        )
        .unwrap();
        assert!(response.deserialize::<bool>().unwrap());

        vault
            .with_conn(|conn| {
                let credentials = repository::get_security_credentials(conn)?;
                assert_eq!(credentials.pin_salt.as_deref(), Some(salt));
                assert_eq!(
                    credentials.pin_hash.as_deref(),
                    Some(expected_hash.as_str())
                );
                assert!(repository::get_master_key_envelope(conn)?.is_none());
                Ok(())
            })
            .unwrap();
        drop(webview);
        drop(app);
        vault.lock().unwrap();
        drop(vault);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn explicit_pin_migration_ipc_returns_usable_recovery_key() {
        let root =
            std::env::temp_dir().join(format!("browsory_pin_migrate_ipc_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let vault = Arc::new(VaultManager::unlocked_fixture(root.clone()));
        let salt = "legacy-migration-salt";
        let old_pin = "1234";
        let expected_hash = hash_pin_with_salt(old_pin, salt);
        vault
            .with_conn(|conn| {
                conn.execute("DELETE FROM master_key_envelope", [])?;
                conn.execute(
                    "UPDATE app_security SET pin_enabled=1, pin_salt=?1, pin_hash=?2, failed_attempts=0, locked_until=0",
                    rusqlite::params![salt, expected_hash],
                )?;
                Ok(())
            })
            .unwrap();

        let state = fixture_state(&root, vault.clone());
        let app = mock_builder()
            .manage(state)
            .invoke_handler(tauri::generate_handler![
                super::set_or_change_pin,
                super::verify_recovery_key
            ])
            .build(mock_context(noop_assets()))
            .unwrap();
        let webview = tauri::WebviewWindowBuilder::new(&app, "migrate-pin-ipc", Default::default())
            .build()
            .unwrap();
        let recovery_key = get_ipc_response(
            &webview,
            ipc_request(
                "set_or_change_pin",
                serde_json::json!({"oldPin": old_pin, "newPin": "5678"}),
            ),
        )
        .unwrap()
        .deserialize::<Option<String>>()
        .unwrap()
        .expect("legacy migration must return the recovery key");
        let recovery_ok = get_ipc_response(
            &webview,
            ipc_request(
                "verify_recovery_key",
                serde_json::json!({"recoveryKey": recovery_key}),
            ),
        )
        .unwrap()
        .deserialize::<bool>()
        .unwrap();
        assert!(recovery_ok);

        vault
            .with_conn(|conn| {
                let credentials = repository::get_security_credentials(conn)?;
                assert!(credentials.pin_hash.is_none());
                assert!(credentials.pin_salt.is_none());
                let envelope_json = repository::get_master_key_envelope(conn)?.unwrap();
                let envelope: MasterKeyEnvelope = serde_json::from_str(&envelope_json).unwrap();
                assert!(unwrap_master_key_with_pin(&envelope, "5678").is_ok());
                Ok(())
            })
            .unwrap();
        drop(webview);
        drop(app);
        vault.lock().unwrap();
        drop(vault);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pin_change_second_write_failure_rolls_back_envelope_and_pin_state() {
        let root = std::env::temp_dir().join(format!(
            "browsory_pin_rollback_ipc_{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let vault = Arc::new(VaultManager::unlocked_fixture(root.clone()));
        let (envelope, _, _) =
            create_master_key_envelope("1234", Utc::now().timestamp_millis()).unwrap();
        let original_json = serde_json::to_string(&envelope).unwrap();
        vault
            .with_conn(|conn| {
                repository::save_master_key_envelope(conn, &original_json, None)?;
                conn.execute(
                    "UPDATE app_security SET pin_enabled=1, pin_salt=NULL, pin_hash=NULL, failed_attempts=0, locked_until=0",
                    [],
                )?;
                conn.execute_batch(
                    "CREATE TRIGGER test_pin_update_failure
                     BEFORE UPDATE OF pin_enabled ON app_security
                     BEGIN SELECT RAISE(ABORT, 'injected second PIN write failure'); END;",
                )?;
                Ok(())
            })
            .unwrap();

        let state = fixture_state(&root, vault.clone());
        let app = mock_builder()
            .manage(state)
            .invoke_handler(tauri::generate_handler![super::set_or_change_pin])
            .build(mock_context(noop_assets()))
            .unwrap();
        let webview =
            tauri::WebviewWindowBuilder::new(&app, "pin-rollback-ipc", Default::default())
                .build()
                .unwrap();
        let response = get_ipc_response(
            &webview,
            ipc_request(
                "set_or_change_pin",
                serde_json::json!({"oldPin": "1234", "newPin": "5678"}),
            ),
        );
        assert!(response.is_err(), "injected second write must fail");

        vault
            .with_conn(|conn| {
                assert_eq!(
                    repository::get_master_key_envelope(conn)?.as_deref(),
                    Some(original_json.as_str())
                );
                let credentials = repository::get_security_credentials(conn)?;
                assert!(credentials.pin_enabled);
                let retained: MasterKeyEnvelope = serde_json::from_str(&original_json).unwrap();
                assert!(unwrap_master_key_with_pin(&retained, "1234").is_ok());
                assert!(unwrap_master_key_with_pin(&retained, "5678").is_err());
                Ok(())
            })
            .unwrap();
        drop(webview);
        drop(app);
        vault.lock().unwrap();
        drop(vault);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn export_atomic_copy_replaces_only_after_complete_write() {
        let root =
            std::env::temp_dir().join(format!("browsory_export_atomic_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("source.browsory-backup");
        let target = root.join("target.browsory-backup");
        let source_bytes = vec![b'x'; 128 * 1024];
        std::fs::write(&source, &source_bytes).unwrap();
        std::fs::write(&target, b"old export").unwrap();
        copy_backup_export_atomic(&source, &target, || Ok(())).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), source_bytes);
        assert!(!std::fs::read_dir(&root).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("browsory-export-")));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn export_atomic_cancel_keeps_old_target_and_removes_partial() {
        let root =
            std::env::temp_dir().join(format!("browsory_export_cancel_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("source.browsory-backup");
        let target = root.join("target.browsory-backup");
        std::fs::write(&source, vec![b'x'; 256 * 1024]).unwrap();
        std::fs::write(&target, b"old export").unwrap();
        let mut checks = 0;
        let result = copy_backup_export_atomic(&source, &target, || {
            checks += 1;
            if checks >= 3 {
                Err("cancelled by test".into())
            } else {
                Ok(())
            }
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"old export");
        assert!(!std::fs::read_dir(&root).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("browsory-export-")));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn export_atomic_publish_failure_preserves_existing_directory() {
        let root = std::env::temp_dir().join(format!(
            "browsory_export_publish_failure_{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("source.browsory-backup");
        let target = root.join("target.browsory-backup");
        std::fs::write(&source, b"backup payload").unwrap();
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("keep"), b"original").unwrap();
        assert!(copy_backup_export_atomic(&source, &target, || Ok(())).is_err());
        assert_eq!(std::fs::read(target.join("keep")).unwrap(), b"original");
        assert!(!std::fs::read_dir(&root).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("browsory-export-")));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_pin_migration_envelope_failure_keeps_legacy_credentials() {
        let root =
            std::env::temp_dir().join(format!("browsory_pin_mig_fail_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let vault = Arc::new(VaultManager::unlocked_fixture(root.clone()));
        let salt = "legacy-fail-salt";
        let old_pin = "1234";
        let expected_hash = hash_pin_with_salt(old_pin, salt);
        vault
            .with_conn(|conn| {
                conn.execute("DELETE FROM master_key_envelope", [])?;
                conn.execute(
                    "UPDATE app_security SET pin_enabled=1, pin_salt=?1, pin_hash=?2, failed_attempts=0, locked_until=0",
                    rusqlite::params![salt, expected_hash],
                )?;
                conn.execute_batch(
                    "CREATE TRIGGER test_envelope_insert_failure
                     BEFORE INSERT ON master_key_envelope
                     BEGIN SELECT RAISE(ABORT, 'injected envelope write failure'); END;",
                )?;
                Ok(())
            })
            .unwrap();

        let state = fixture_state(&root, vault.clone());
        let app = mock_builder()
            .manage(state)
            .invoke_handler(tauri::generate_handler![
                super::get_security_state,
                super::set_or_change_pin,
                super::verify_pin
            ])
            .build(mock_context(noop_assets()))
            .unwrap();
        let webview = tauri::WebviewWindowBuilder::new(&app, "legacy-fail-ipc", Default::default())
            .build()
            .unwrap();

        let security_state = get_ipc_response(
            &webview,
            ipc_request("get_security_state", serde_json::json!({})),
        )
        .unwrap()
        .deserialize::<SecurityState>()
        .unwrap();
        assert!(security_state.pin_enabled);
        assert!(security_state.pin_migration_needed);

        let mig_err = get_ipc_response(
            &webview,
            ipc_request(
                "set_or_change_pin",
                serde_json::json!({"oldPin": old_pin, "newPin": "5678"}),
            ),
        );
        assert!(mig_err.is_err());

        let verified = get_ipc_response(
            &webview,
            ipc_request("verify_pin", serde_json::json!({"pin": old_pin})),
        )
        .unwrap()
        .deserialize::<bool>()
        .unwrap();
        assert!(verified);

        drop(webview);
        drop(app);
        vault.lock().unwrap();
        drop(vault);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reset_and_disable_pin_roll_back_atomically_on_second_write_failure() {
        let root = std::env::temp_dir().join(format!(
            "browsory_pin_reset_disable_{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let vault = Arc::new(VaultManager::unlocked_fixture(root.clone()));
        let (envelope, recovery_key, _) =
            create_master_key_envelope("1234", Utc::now().timestamp_millis()).unwrap();
        let original_json = serde_json::to_string(&envelope).unwrap();
        vault
            .with_conn(|conn| {
                repository::save_master_key_envelope(conn, &original_json, None)?;
                conn.execute(
                    "UPDATE app_security SET pin_enabled=1, pin_salt=NULL, pin_hash=NULL, failed_attempts=2, locked_until=0",
                    [],
                )?;
                conn.execute_batch(
                    "CREATE TRIGGER test_reset_pin_failure
                     BEFORE UPDATE OF failed_attempts ON app_security
                     WHEN NEW.failed_attempts = 0
                     BEGIN SELECT RAISE(ABORT, 'injected reset failure'); END;",
                )?;
                Ok(())
            })
            .unwrap();

        let state = fixture_state(&root, vault.clone());
        let app = mock_builder()
            .manage(state)
            .invoke_handler(tauri::generate_handler![
                super::reset_pin_with_recovery_key,
                super::disable_pin,
                super::verify_pin
            ])
            .build(mock_context(noop_assets()))
            .unwrap();
        let webview =
            tauri::WebviewWindowBuilder::new(&app, "reset-disable-ipc", Default::default())
                .build()
                .unwrap();

        assert!(get_ipc_response(
            &webview,
            ipc_request(
                "reset_pin_with_recovery_key",
                serde_json::json!({"recoveryKey": recovery_key, "newPin": "9999"}),
            ),
        )
        .is_err());

        vault
            .with_conn(|conn| {
                assert_eq!(
                    repository::get_master_key_envelope(conn)?.as_deref(),
                    Some(original_json.as_str())
                );
                let retained: MasterKeyEnvelope = serde_json::from_str(&original_json).unwrap();
                assert!(unwrap_master_key_with_pin(&retained, "1234").is_ok());
                assert!(unwrap_master_key_with_pin(&retained, "9999").is_err());

                conn.execute_batch("DROP TRIGGER test_reset_pin_failure;")?;
                conn.execute_batch(
                    "CREATE TRIGGER test_disable_pin_envelope_failure
                     BEFORE DELETE ON master_key_envelope
                     BEGIN SELECT RAISE(ABORT, 'injected envelope delete failure'); END;",
                )?;
                Ok(())
            })
            .unwrap();

        assert!(get_ipc_response(
            &webview,
            ipc_request("disable_pin", serde_json::json!({"currentPin": "1234"})),
        )
        .is_err());

        vault
            .with_conn(|conn| {
                let creds = repository::get_security_credentials(conn)?;
                assert!(creds.pin_enabled);
                assert!(repository::get_master_key_envelope(conn)?.is_some());
                Ok(())
            })
            .unwrap();

        drop(webview);
        drop(app);
        vault.lock().unwrap();
        drop(vault);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn orphan_backup_reconciliation_and_sixteen_real_backup_rotation() {
        let root = std::env::temp_dir().join(format!(
            "browsory_backup_orphan_rot_{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let vault = Arc::new(VaultManager::unlocked_fixture(root.clone()));
        let backups_dir = root.join("backups");

        vault
            .with_conn(|conn| {
                conn.execute_batch(
                    "CREATE TRIGGER test_orphan_insert_fail
                     BEFORE INSERT ON backups
                     BEGIN SELECT RAISE(ABORT, 'injected registration failure'); END;",
                )?;
                Ok(())
            })
            .unwrap();

        let state = fixture_state(&root, vault.clone());
        let app = mock_builder()
            .manage(state)
            .invoke_handler(tauri::generate_handler![
                super::create_database_backup,
                super::list_backups,
                super::export_database_backup
            ])
            .build(mock_context(noop_assets()))
            .unwrap();
        let webview =
            tauri::WebviewWindowBuilder::new(&app, "backup-orphan-rot-ipc", Default::default())
                .build()
                .unwrap();

        assert!(get_ipc_response(
            &webview,
            ipc_request("create_database_backup", serde_json::json!({"notes": null})),
        )
        .is_err());

        vault
            .with_conn(|conn| {
                conn.execute_batch("DROP TRIGGER test_orphan_insert_fail;")?;
                Ok(())
            })
            .unwrap();

        let listed = get_ipc_response(&webview, ipc_request("list_backups", serde_json::json!({})))
            .unwrap()
            .deserialize::<Vec<BackupInfo>>()
            .unwrap();
        assert_eq!(listed.len(), 1);
        assert!(
            !std::fs::read_dir(&backups_dir).unwrap().any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".orphan.json")),
            "resolved orphan marker must be cleaned up"
        );

        let exported_target = root.join("exported-target.browsory-backup");
        std::fs::write(&exported_target, b"existing destination").unwrap();
        get_ipc_response(
            &webview,
            ipc_request(
                "export_database_backup",
                serde_json::json!({"backupId": listed[0].id, "targetFilePath": exported_target}),
            ),
        )
        .unwrap();

        let readonly_target = root.join("readonly-target.browsory-backup");
        std::fs::write(&readonly_target, b"protected original export").unwrap();
        let before_sha = crate::security::backup::compute_file_sha256(&readonly_target).unwrap();
        let mut perms = std::fs::metadata(&readonly_target).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&readonly_target, perms).unwrap();
        assert!(get_ipc_response(
            &webview,
            ipc_request(
                "export_database_backup",
                serde_json::json!({"backupId": listed[0].id, "targetFilePath": readonly_target}),
            ),
        )
        .is_err());
        assert_eq!(
            crate::security::backup::compute_file_sha256(&readonly_target).unwrap(),
            before_sha
        );
        #[cfg(windows)]
        {
            let _ = std::process::Command::new("attrib")
                .arg("-R")
                .arg(&readonly_target)
                .status();
        }
        #[cfg(not(windows))]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms_rw = std::fs::metadata(&readonly_target).unwrap().permissions();
            perms_rw.set_mode(0o644);
            let _ = std::fs::set_permissions(&readonly_target, perms_rw);
        }

        for idx in 1..=15 {
            get_ipc_response(
                &webview,
                ipc_request(
                    "create_database_backup",
                    serde_json::json!({"notes": format!("rot-{idx}")}),
                ),
            )
            .unwrap();
        }
        let after_rotation =
            get_ipc_response(&webview, ipc_request("list_backups", serde_json::json!({})))
                .unwrap()
                .deserialize::<Vec<BackupInfo>>()
                .unwrap();
        assert_eq!(after_rotation.len(), 15);
        assert!(
            !after_rotation.iter().any(|b| b.id == listed[0].id),
            "oldest orphan-reconciled backup must be rotated out after 16th backup"
        );
        let disk_packages: Vec<_> = std::fs::read_dir(&backups_dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".browsory-backup")
            })
            .collect();
        assert_eq!(disk_packages.len(), 15);

        let restore_dir = root.join("restore-check");
        std::fs::create_dir_all(&restore_dir).unwrap();
        let restore_vault = VaultManager::new(restore_dir.clone());
        let _ = restore_vault
            .create_vault("Synthetic-fixture-password")
            .unwrap();
        let old_key = restore_vault.master_key().unwrap();
        let old_env = restore_vault.envelope().unwrap();
        restore_vault.lock().unwrap();
        let (restored_key, _restored_env) = crate::security::backup::restore_encrypted_backup_flow(
            &exported_target,
            &restore_dir,
            Some("Synthetic-fixture-password"),
            None,
            Some(&old_key),
            Some(&old_env),
            || Ok(()),
        )
        .unwrap();
        drop(restored_key);
        drop(restore_vault);

        drop(webview);
        drop(app);
        vault.lock().unwrap();
        drop(vault);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn vault_status_is_nonblocking_and_queued_settings_reject_after_lock() {
        let root =
            std::env::temp_dir().join(format!("browsory_nonblock_status_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let vault = Arc::new(VaultManager::unlocked_fixture(root.clone()));
        let state = fixture_state(&root, vault.clone());
        let app = mock_builder()
            .manage(state.clone())
            .invoke_handler(tauri::generate_handler![
                super::get_vault_status,
                crate::commands::settings::get_setting
            ])
            .build(mock_context(noop_assets()))
            .unwrap();
        let webview =
            tauri::WebviewWindowBuilder::new(&app, "nonblock-status-ipc", Default::default())
                .build()
                .unwrap();

        let conn_arc = vault.connection().unwrap();
        let guard = conn_arc.lock().unwrap();

        let status = get_ipc_response(
            &webview,
            ipc_request("get_vault_status", serde_json::json!({})),
        )
        .unwrap()
        .deserialize::<VaultStatusInfo>()
        .unwrap();
        assert_eq!(status.status, VaultStatus::Unlocked);

        let op_before_lock = state.begin_operation().unwrap();
        let probe = op_before_lock.cancellation_probe();
        let op_worker = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while op_before_lock.check().is_ok() {
                assert!(
                    std::time::Instant::now() < deadline,
                    "timed out waiting for vault.lock() to atomically revoke in-flight operation"
                );
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        });
        drop(guard);
        vault.lock().unwrap();
        op_worker.join().unwrap();
        assert!(probe());

        assert!(get_ipc_response(
            &webview,
            ipc_request("get_setting", serde_json::json!({"key": "ai_model"})),
        )
        .is_err());

        vault
            .unlock_with_password("Synthetic-fixture-password")
            .unwrap();
        assert!(
            probe(),
            "revoked operation must never become valid across a new session"
        );

        drop(webview);
        drop(app);
        vault.lock().unwrap();
        drop(vault);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pin_change_and_disable_count_failures_and_honor_cooldown() {
        let root = std::env::temp_dir().join(format!(
            "browsory_pin_mutation_lockout_{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let vault = Arc::new(VaultManager::unlocked_fixture(root.clone()));
        let (envelope, _, _) =
            create_master_key_envelope("1234", Utc::now().timestamp_millis()).unwrap();
        let envelope_json = serde_json::to_string(&envelope).unwrap();
        vault
            .with_conn(|conn| {
                repository::save_master_key_envelope(conn, &envelope_json, None)?;
                conn.execute(
                    "UPDATE app_security SET pin_enabled=1, pin_salt=NULL, pin_hash=NULL, failed_attempts=0, locked_until=0",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        let state = fixture_state(&root, vault.clone());
        let app = mock_builder()
            .manage(state)
            .invoke_handler(tauri::generate_handler![
                super::set_or_change_pin,
                super::disable_pin
            ])
            .build(mock_context(noop_assets()))
            .unwrap();
        let webview =
            tauri::WebviewWindowBuilder::new(&app, "pin-mutation-lockout-ipc", Default::default())
                .build()
                .unwrap();

        assert!(get_ipc_response(
            &webview,
            ipc_request(
                "set_or_change_pin",
                serde_json::json!({"oldPin": "9999", "newPin": "5678"}),
            ),
        )
        .is_err());
        vault
            .with_conn(|conn| {
                let creds = repository::get_security_credentials(conn)?;
                assert_eq!(creds.failed_attempts, 1);
                assert_eq!(creds.locked_until, 0);
                Ok(())
            })
            .unwrap();

        let cooldown = Utc::now().timestamp_millis() + 60_000;
        vault
            .with_conn(|conn| {
                conn.execute(
                    "UPDATE app_security SET locked_until=?1",
                    rusqlite::params![cooldown],
                )?;
                Ok(())
            })
            .unwrap();
        assert!(get_ipc_response(
            &webview,
            ipc_request(
                "set_or_change_pin",
                serde_json::json!({"oldPin": "1234", "newPin": "5678"}),
            ),
        )
        .is_err());

        vault
            .with_conn(|conn| {
                let creds = repository::get_security_credentials(conn)?;
                assert_eq!(creds.failed_attempts, 1);
                Ok(())
            })
            .unwrap();

        vault
            .with_conn(|conn| {
                conn.execute("UPDATE app_security SET locked_until=0", [])?;
                Ok(())
            })
            .unwrap();
        assert!(get_ipc_response(
            &webview,
            ipc_request("disable_pin", serde_json::json!({"currentPin": "9999"}),),
        )
        .is_err());
        vault
            .with_conn(|conn| {
                let creds = repository::get_security_credentials(conn)?;
                assert_eq!(creds.failed_attempts, 2);
                Ok(())
            })
            .unwrap();

        vault
            .with_conn(|conn| {
                conn.execute(
                    "UPDATE app_security SET locked_until=?1",
                    rusqlite::params![Utc::now().timestamp_millis() + 60_000],
                )?;
                Ok(())
            })
            .unwrap();
        assert!(get_ipc_response(
            &webview,
            ipc_request("disable_pin", serde_json::json!({"currentPin": "1234"}),),
        )
        .is_err());

        drop(webview);
        drop(app);
        vault.lock().unwrap();
        drop(vault);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn passive_dpapi_resume_ipc_enforces_four_digit_pin_and_cooldown() {
        let root =
            std::env::temp_dir().join(format!("browsory_passive_ipc_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let vault = Arc::new(VaultManager::new(root.clone()));
        vault.ensure_passive_vault_unlocked().unwrap();
        let recovery_key = vault
            .regenerate_unconfirmed_recovery_key_from_session()
            .unwrap();
        vault.confirm_recovery_key(&recovery_key).unwrap();
        let state = fixture_state(&root, vault.clone());
        let app = mock_builder()
            .manage(state)
            .invoke_handler(tauri::generate_handler![
                super::get_vault_status,
                super::set_or_change_pin,
                super::resume_passive_vault,
                super::lock_vault
            ])
            .build(mock_context(noop_assets()))
            .unwrap();
        let webview =
            tauri::WebviewWindowBuilder::new(&app, "passive-dpapi-ipc", Default::default())
                .build()
                .unwrap();

        let status = get_ipc_response(
            &webview,
            ipc_request("get_vault_status", serde_json::json!({})),
        )
        .unwrap()
        .deserialize::<super::VaultStatusInfo>()
        .unwrap();
        assert_eq!(status.status, VaultStatus::Unlocked);
        assert!(status.has_dpapi_wrap);
        assert!(!status.recovery_key_pending);

        // Non-4-digit PINs must be rejected.
        assert!(get_ipc_response(
            &webview,
            ipc_request("set_or_change_pin", serde_json::json!({"newPin": "123456"})),
        )
        .is_err());

        // Set 4-digit PIN cleanly.
        assert!(get_ipc_response(
            &webview,
            ipc_request("set_or_change_pin", serde_json::json!({"newPin": "2580"})),
        )
        .is_ok());

        // Lock and verify resume requires PIN, rejects wrong PIN, and unlocks with 4-digit PIN.
        assert!(
            get_ipc_response(&webview, ipc_request("lock_vault", serde_json::json!({}))).is_ok()
        );
        assert_eq!(vault.status(), VaultStatus::Locked);

        let missing = get_ipc_response(
            &webview,
            ipc_request("resume_passive_vault", serde_json::json!({})),
        )
        .unwrap()
        .deserialize::<crate::security::vault::PassiveResumeResult>()
        .unwrap();
        assert!(!missing.unlocked);
        assert!(missing.pin_required);
        assert_eq!(vault.status(), VaultStatus::Locked);

        let wrong = get_ipc_response(
            &webview,
            ipc_request("resume_passive_vault", serde_json::json!({"pin": "0000"})),
        )
        .unwrap()
        .deserialize::<crate::security::vault::PassiveResumeResult>()
        .unwrap();
        assert!(!wrong.unlocked);
        assert!(wrong.pin_required);
        assert!(wrong.error_message.is_some());
        assert_eq!(vault.status(), VaultStatus::Locked);

        let ok = get_ipc_response(
            &webview,
            ipc_request("resume_passive_vault", serde_json::json!({"pin": "2580"})),
        )
        .unwrap()
        .deserialize::<crate::security::vault::PassiveResumeResult>()
        .unwrap();
        assert!(ok.unlocked);
        assert!(!ok.pin_required);
        assert_eq!(vault.status(), VaultStatus::Unlocked);

        drop(webview);
        drop(app);
        vault.lock().unwrap();
        drop(vault);
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn locked_passive_dpapi_vault_resets_pin_with_recovery_key_ipc() {
        let root = std::env::temp_dir().join(format!(
            "browsory_passive_pin_reset_ipc_{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let vault = Arc::new(VaultManager::new(root.clone()));
        vault.ensure_passive_vault_unlocked().unwrap();
        let state = fixture_state(&root, vault.clone());
        let app = mock_builder()
            .manage(state)
            .invoke_handler(tauri::generate_handler![
                super::set_or_change_pin,
                super::lock_vault,
                super::resume_passive_vault,
                super::reset_pin_with_recovery_key
            ])
            .build(mock_context(noop_assets()))
            .unwrap();
        let webview =
            tauri::WebviewWindowBuilder::new(&app, "passive-pin-reset-ipc", Default::default())
                .build()
                .unwrap();

        let rec_key = get_ipc_response(
            &webview,
            ipc_request("set_or_change_pin", serde_json::json!({"newPin": "2580"})),
        )
        .unwrap()
        .deserialize::<Option<String>>()
        .unwrap()
        .expect("setting PIN returns recovery key");

        get_ipc_response(&webview, ipc_request("lock_vault", serde_json::json!({}))).unwrap();
        assert_eq!(vault.status(), VaultStatus::Locked);

        // Wrong recovery key must fail and leave vault locked.
        assert!(get_ipc_response(
            &webview,
            ipc_request(
                "reset_pin_with_recovery_key",
                serde_json::json!({"recoveryKey": "BHA-0000-0000-0000", "newPin": "1357"}),
            ),
        )
        .is_err());
        assert_eq!(vault.status(), VaultStatus::Locked);

        // Correct recovery key resets PIN while locked and unlocks the vault.
        get_ipc_response(
            &webview,
            ipc_request(
                "reset_pin_with_recovery_key",
                serde_json::json!({"recoveryKey": rec_key, "newPin": "1357"}),
            ),
        )
        .unwrap();
        assert_eq!(vault.status(), VaultStatus::Unlocked);

        // Lock again and confirm the new PIN works while old PIN is rejected.
        get_ipc_response(&webview, ipc_request("lock_vault", serde_json::json!({}))).unwrap();
        let old_res = get_ipc_response(
            &webview,
            ipc_request("resume_passive_vault", serde_json::json!({"pin": "2580"})),
        )
        .unwrap()
        .deserialize::<crate::security::vault::PassiveResumeResult>()
        .unwrap();
        assert!(!old_res.unlocked);

        let new_res = get_ipc_response(
            &webview,
            ipc_request("resume_passive_vault", serde_json::json!({"pin": "1357"})),
        )
        .unwrap()
        .deserialize::<crate::security::vault::PassiveResumeResult>()
        .unwrap();
        assert!(new_res.unlocked);

        drop(webview);
        drop(app);
        vault.lock().unwrap();
        drop(vault);
        let _ = std::fs::remove_dir_all(root);
    }
}
