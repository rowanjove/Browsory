use super::session::{ConnectionHandle, ReadConnectionHandle, VaultOperation, VaultSession};
use chrono::Utc;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use tracing::{info, warn};

use super::envelope::{
    change_envelope_password, enroll_dpapi_wrap, rotate_recovery_wrap,
    unwrap_master_key_with_dpapi, unwrap_master_key_with_password, unwrap_master_key_with_recovery,
    MasterKey, VaultEnvelope,
};
use crate::database::encrypt_migration::{
    create_passive_vault_database, create_vault_database, is_plain_sqlite_database,
    migrate_plaintext_to_sqlcipher, migrate_plaintext_to_sqlcipher_passive,
    recover_from_interrupted_migration,
};
use crate::database::migrations::run_migrations;
use crate::database::sqlcipher::{
    open_encrypted_database, open_encrypted_database_read_only, open_encrypted_database_with_cache,
};
use crate::error::{AppError, AppResult};

pub(crate) struct HeadlessVaultSession {
    conn: Option<Connection>,
    _master_key: MasterKey,
}

impl HeadlessVaultSession {
    pub(crate) fn connection_mut(&mut self) -> &mut Connection {
        self.conn
            .as_mut()
            .expect("headless vault connection active")
    }

    #[cfg(test)]
    pub(crate) fn connection(&self) -> &Connection {
        self.conn
            .as_ref()
            .expect("headless vault connection active")
    }
}

impl Drop for HeadlessVaultSession {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            let _ = conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);");
            drop(conn);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PassiveResumeResult {
    pub unlocked: bool,
    pub pin_required: bool,
    pub is_locked_out: bool,
    pub lockout_remaining_secs: i64,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PassivePinCheck {
    AllowUnlock,
    PinRequired,
    InvalidPin {
        is_locked_out: bool,
        lockout_remaining_secs: i64,
        message: String,
    },
    LockedOut {
        remaining_secs: i64,
        message: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultStatus {
    /// No database and no envelope: first startup wizard
    Unconfigured,
    /// Legacy plain SQLite database exists without a vault envelope
    LegacyPlaintext,
    /// Encrypted vault exists and is currently locked (no active DB connection)
    Locked,
    /// New access is revoked; active operations and connection are draining.
    Locking,
    /// Unlocking in progress (deriving Argon2id key)
    Unlocking,
    /// Successfully unlocked; secure database connection is active
    Unlocked,
    /// Actively migrating from plaintext or restoring backup
    Migrating,
    /// Security or state inconsistency error
    Error(String),
}

/// Thread-safe controller for the database vault lifecycle.
/// Enforces backend access control by guarding connection ownership.
pub struct VaultManager {
    data_dir: PathBuf,
    status: RwLock<VaultStatus>,
    envelope: RwLock<Option<VaultEnvelope>>,
    active_conn: RwLock<Option<Arc<VaultSession>>>,
    master_key: RwLock<Option<MasterKey>>,
    lifecycle: Mutex<()>,
    // One atomic word: bit 0 is a pending lock; other bits are its generation.
    revocation: Arc<AtomicU64>,
}

impl VaultManager {
    #[cfg(test)]
    pub(crate) fn unlocked_fixture(data_dir: PathBuf) -> Self {
        use super::envelope::{create_vault_envelope, KdfParameters, MIN_MEM_COST_KB};
        let params = KdfParameters {
            mem_cost_kb: MIN_MEM_COST_KB,
            time_cost: 1,
            parallelism: 1,
        };
        let (mut envelope, _, key) =
            create_vault_envelope("Synthetic-fixture-password", Some(params), 0).unwrap();
        envelope.recovery_key_confirmed = true;
        envelope.save_atomic(&data_dir).unwrap();
        let mut conn =
            open_encrypted_database(&data_dir.join("archive.db"), key.as_bytes(), true).unwrap();
        run_migrations(&mut conn).unwrap();
        let manager = Self::new(data_dir);
        manager.set_unlocked(conn, key, envelope).unwrap();
        manager
    }

    /// Initializes a new VaultManager inspecting the files in data_dir
    pub fn new(data_dir: PathBuf) -> Self {
        let manager = Self {
            data_dir,
            status: RwLock::new(VaultStatus::Unconfigured),
            envelope: RwLock::new(None),
            active_conn: RwLock::new(None),
            master_key: RwLock::new(None),
            lifecycle: Mutex::new(()),
            revocation: Arc::new(AtomicU64::new(0)),
        };
        manager.refresh_state();
        manager
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("archive.db")
    }

    pub fn envelope_path(&self) -> PathBuf {
        VaultEnvelope::envelope_path(&self.data_dir)
    }

    pub fn status(&self) -> VaultStatus {
        let status = self.status.read().unwrap().clone();
        if self.revocation.load(Ordering::Acquire) & 1 != 0
            && matches!(status, VaultStatus::Unlocked | VaultStatus::Unlocking)
        {
            VaultStatus::Locking
        } else {
            status
        }
    }

    pub fn is_unlocked(&self) -> bool {
        matches!(self.status(), VaultStatus::Unlocked)
    }

    /// Revoke access immediately without waiting for lifecycle, SQL or task drain.
    /// A platform callback must follow this with `lock()` on a blocking worker.
    /// The install guard prevents an in-flight unlock from publishing a usable
    /// new session after the power event.
    pub(crate) fn revoke_active_session(&self) {
        // Set pending and advance generation as one indivisible operation.
        // No application locks, waker enumeration or I/O on native callbacks.
        let _ = self
            .revocation
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |state| {
                Some(state.wrapping_add(2) | 1)
            });
    }

    /// Capture before queueing work, so old unlocks cannot reopen after a lock.
    pub(crate) fn request_generation(&self) -> Result<u64, String> {
        let generation = self.revocation.load(Ordering::Acquire);
        self.check_generation(generation)?;
        Ok(generation)
    }

    pub(crate) fn check_generation(&self, expected: u64) -> Result<(), String> {
        if expected != self.revocation.load(Ordering::Acquire)
            || self.revocation.load(Ordering::Acquire) & 1 != 0
        {
            Err("操作已被较新的锁定请求取消，请重新操作".into())
        } else {
            Ok(())
        }
    }

    pub fn envelope(&self) -> Option<VaultEnvelope> {
        self.envelope.read().unwrap().clone()
    }

    pub fn master_key(&self) -> Option<MasterKey> {
        self.master_key.read().unwrap().clone()
    }

    /// Evaluates filesystem artifacts to establish the initial VaultStatus on startup
    fn refresh_state(&self) {
        if let Err(error) = super::restore::recover(&self.data_dir) {
            *self.status.write().unwrap() =
                VaultStatus::Error(format!("备份恢复未完成，已保留现场: {error}"));
            return;
        }
        // 0. Automatically recover from any interrupted migration state before inspecting artifacts
        if let Err(error) = recover_from_interrupted_migration(&self.data_dir) {
            *self.status.write().unwrap() =
                VaultStatus::Error(format!("迁移恢复未完成，已保留数据: {}", error));
            return;
        }

        let db_path = self.db_path();
        let _env_path = self.envelope_path();

        // 1. Check if an unfinished migration recovery file still persists abnormally
        let staging_path = self.data_dir.join("archive.staging.db");
        if staging_path.exists() {
            warn!("发现未完成的迁移临时文件: {:?}", staging_path);
            *self.status.write().unwrap() = VaultStatus::Error(
                "检测到此前未正常完成的数据库迁移，且自动自愈失败，请检查文件权限或从备份恢复"
                    .into(),
            );
            return;
        }

        // 2. Load envelope if present
        let envelope_res = VaultEnvelope::load_from_dir(&self.data_dir);
        let envelope_opt = match envelope_res {
            Ok(env) => env,
            Err(e) => {
                *self.status.write().unwrap() =
                    VaultStatus::Error(format!("保险库元数据损坏: {}", e));
                return;
            }
        };

        if let Some(env) = envelope_opt {
            *self.envelope.write().unwrap() = Some(env);
            if db_path.exists() {
                // If envelope exists and archive.db is plain SQLite, metadata state mismatch!
                if is_plain_sqlite_database(&db_path) {
                    *self.status.write().unwrap() =
                        VaultStatus::Error("存在加密信封但数据库文件为明文格式，状态不一致".into());
                    return;
                }
                *self.status.write().unwrap() = VaultStatus::Locked;
            } else {
                // Envelope exists but db file missing
                *self.status.write().unwrap() =
                    VaultStatus::Error("保险库信封存在但数据库文件缺失，请从备份恢复".into());
            }
        } else {
            // No envelope
            *self.envelope.write().unwrap() = None;
            if db_path.exists() {
                if is_plain_sqlite_database(&db_path) {
                    *self.status.write().unwrap() = VaultStatus::LegacyPlaintext;
                } else {
                    *self.status.write().unwrap() = VaultStatus::Error(
                        "数据库为加密文件但缺少必要的保险库元数据 (vault.envelope.json)".into(),
                    );
                }
            } else {
                *self.status.write().unwrap() = VaultStatus::Unconfigured;
            }
        }
    }

    /// Register work against the current unlock. The operation cannot migrate
    /// into a future session and is drained before lock returns.
    pub fn begin_operation(&self) -> AppResult<VaultOperation> {
        if !self.is_unlocked() {
            return Err(AppError::VaultLocked("数据库保险库未解锁".into()));
        }
        self.active_conn
            .read()
            .unwrap()
            .as_ref()
            .ok_or_else(|| AppError::VaultLocked("活动数据库连接不存在".into()))?
            .operation()
    }

    pub fn with_conn<F, R>(&self, f: F) -> AppResult<R>
    where
        F: FnOnce(&Connection) -> AppResult<R>,
    {
        self.begin_operation()?.with_conn(f)
    }

    pub fn with_conn_mut<F, R>(&self, f: F) -> AppResult<R>
    where
        F: FnOnce(&mut Connection) -> AppResult<R>,
    {
        self.begin_operation()?.with_conn_mut(f)
    }

    pub fn with_read_conn<F, R>(&self, f: F) -> AppResult<R>
    where
        F: FnOnce(&Connection) -> AppResult<R>,
    {
        self.begin_operation()?.with_read_conn(f)
    }

    pub fn connection(&self) -> AppResult<ConnectionHandle> {
        if !self.is_unlocked() {
            return Err(AppError::VaultLocked("数据库保险库未解锁".into()));
        }
        self.active_conn
            .read()
            .unwrap()
            .as_ref()
            .ok_or_else(|| AppError::VaultLocked("活动数据库连接不存在".into()))?
            .handle()
    }

    pub fn read_connection(&self) -> AppResult<ReadConnectionHandle> {
        if !self.is_unlocked() {
            return Err(AppError::VaultLocked("数据库保险库未解锁".into()));
        }
        self.active_conn
            .read()
            .unwrap()
            .as_ref()
            .ok_or_else(|| AppError::VaultLocked("活动数据库连接不存在".into()))?
            .read_handle()
    }

    fn open_read_pool(&self, master_key: &MasterKey) -> Result<Vec<Connection>, String> {
        const READ_POOL_SIZE: usize = 4;
        let db_path = self.db_path();
        if !db_path.is_file() {
            return Ok(Vec::new());
        }
        let mut readers = Vec::with_capacity(READ_POOL_SIZE);
        for _ in 0..READ_POOL_SIZE {
            let reader = open_encrypted_database_read_only(&db_path, master_key.as_bytes())
                .map_err(|e| format!("初始化并发只读数据库连接失败: {e}"))?;
            readers.push(reader);
        }
        Ok(readers)
    }

    /// Transitions vault into Unlocked state with the provided active connection and master key
    fn set_unlocked(
        &self,
        conn: Connection,
        master_key: MasterKey,
        envelope: VaultEnvelope,
    ) -> Result<(), String> {
        self.set_unlocked_at_generation(conn, master_key, envelope, self.request_generation()?)
    }

    fn set_unlocked_at_generation(
        &self,
        conn: Connection,
        master_key: MasterKey,
        envelope: VaultEnvelope,
        request_generation: u64,
    ) -> Result<(), String> {
        if let Err(error) = self.check_generation(request_generation) {
            drop(conn);
            drop(master_key);
            *self.envelope.write().unwrap() = Some(envelope);
            *self.status.write().unwrap() = VaultStatus::Locked;
            return Err(error);
        }
        let readers = match self.open_read_pool(&master_key) {
            Ok(readers) => readers,
            Err(error) => {
                drop(conn);
                drop(master_key);
                *self.envelope.write().unwrap() = Some(envelope);
                *self.status.write().unwrap() = VaultStatus::Locked;
                return Err(error);
            }
        };
        let session =
            VaultSession::with_revocation_and_readers(conn, readers, self.revocation.clone())
                .map_err(|error| {
                    let message = format!("无法初始化可撤销数据库会话: {error}");
                    *self.status.write().unwrap() = VaultStatus::Error(message.clone());
                    message
                })?;
        let mut active = self.active_conn.write().unwrap();
        if self.check_generation(request_generation).is_err() {
            drop(session);
            *self.envelope.write().unwrap() = Some(envelope);
            *self.status.write().unwrap() = VaultStatus::Locked;
            return Err("解锁期间收到锁定请求，保险库保持关闭".into());
        }
        *active = Some(session);
        *self.master_key.write().unwrap() = Some(master_key);
        *self.envelope.write().unwrap() = Some(envelope);
        *self.status.write().unwrap() = VaultStatus::Unlocked;
        drop(active);
        info!("数据库保险库已成功解锁");
        Ok(())
    }

    /// Creates a brand new vault: creates MasterKey + RecoveryKey + Envelope, opens DB, and unlocks.
    pub fn create_vault(&self, password: &str) -> Result<String, String> {
        self.create_vault_at_generation(password, self.request_generation()?)
    }

    pub(crate) fn create_vault_at_generation(
        &self,
        password: &str,
        request_generation: u64,
    ) -> Result<String, String> {
        let _lifecycle = self
            .lifecycle
            .lock()
            .map_err(|_| "保险库生命周期锁不可用")?;
        self.check_generation(request_generation)?;
        let current_status = self.status();
        if current_status != VaultStatus::Unconfigured {
            return Err(format!("保险库已配置或处于异常状态 ({:?})", current_status));
        }

        let (envelope, recovery_key_str, master_key) =
            match create_vault_database(&self.data_dir, password) {
                Ok(created) => created,
                Err(error) => {
                    self.refresh_state();
                    return Err(format!("创建保险库失败: {}", error));
                }
            };

        // Open encrypted database with master key and run migrations
        let conn = open_encrypted_database(&self.db_path(), master_key.as_bytes(), false).map_err(
            |e| {
                self.refresh_state();
                format!("初始化加密数据库失败: {}", e)
            },
        )?;

        self.set_unlocked_at_generation(conn, master_key, envelope, request_generation)?;
        Ok(recovery_key_str)
    }

    pub fn regenerate_unconfirmed_recovery_key(&self, password: &str) -> Result<String, String> {
        let _lifecycle = self
            .lifecycle
            .lock()
            .map_err(|_| "保险库生命周期锁不可用")?;
        if !self.is_unlocked() {
            return Err("请先用口令解锁保险库".into());
        }
        let mut envelope = self.envelope().ok_or("保险库元数据缺失")?;
        if envelope.recovery_key_confirmed {
            return Err("恢复密钥已经确认，无需重发".into());
        }
        let key = unwrap_master_key_with_password(&envelope, password)?;
        let active_key = self.master_key().ok_or("保险库主密钥不可用")?;
        if key.as_bytes() != active_key.as_bytes() {
            return Err("保险库密钥与信封不匹配".into());
        }
        let display = rotate_recovery_wrap(&mut envelope, &key, Utc::now().timestamp_millis())?;
        envelope.save_atomic(&self.data_dir)?;
        *self.envelope.write().unwrap() = Some(envelope);
        Ok(display)
    }

    /// Reissues an unconfirmed recovery key for a passive vault using the
    /// already-unlocked session key. Passive vaults do not retain a passphrase.
    pub fn regenerate_unconfirmed_recovery_key_from_session(&self) -> Result<String, String> {
        let _lifecycle = self
            .lifecycle
            .lock()
            .map_err(|_| "保险库生命周期锁不可用")?;
        if !self.is_unlocked() {
            return Err("请先解锁保险库".into());
        }
        let mut envelope = self.envelope().ok_or("保险库元数据缺失")?;
        if envelope.recovery_key_confirmed {
            return Err("恢复密钥已经确认，无需重发".into());
        }
        let active_key = self.master_key().ok_or("保险库主密钥不可用")?;
        let display =
            rotate_recovery_wrap(&mut envelope, &active_key, Utc::now().timestamp_millis())?;
        envelope.save_atomic(&self.data_dir)?;
        *self.envelope.write().unwrap() = Some(envelope);
        Ok(display)
    }

    pub fn confirm_recovery_key(&self, recovery_key: &str) -> Result<bool, String> {
        let _lifecycle = self
            .lifecycle
            .lock()
            .map_err(|_| "保险库生命周期锁不可用")?;
        if !self.is_unlocked() {
            return Err("请先解锁保险库".into());
        }
        let mut envelope = self.envelope().ok_or("保险库元数据缺失")?;
        let key = unwrap_master_key_with_recovery(&envelope, recovery_key)?;
        let active_key = self.master_key().ok_or("保险库主密钥不可用")?;
        if key.as_bytes() != active_key.as_bytes() {
            return Err("恢复密钥与保险库不匹配".into());
        }
        if envelope.recovery_key_confirmed {
            return Ok(false);
        }
        envelope.recovery_key_confirmed = true;
        envelope.updated_at = Utc::now().timestamp_millis();
        envelope.save_atomic(&self.data_dir)?;
        *self.envelope.write().unwrap() = Some(envelope);
        Ok(true)
    }

    /// Unlocks the vault using the user password
    pub fn unlock_with_password(&self, password: &str) -> Result<(), String> {
        self.unlock_with_password_at_generation(password, self.request_generation()?)
    }

    pub(crate) fn unlock_with_password_at_generation(
        &self,
        password: &str,
        request_generation: u64,
    ) -> Result<(), String> {
        let _lifecycle = self
            .lifecycle
            .lock()
            .map_err(|_| "保险库生命周期锁不可用")?;
        self.check_generation(request_generation)?;
        let current_status = self.status();
        if current_status == VaultStatus::Unlocked {
            return Ok(());
        }
        if current_status != VaultStatus::Locked {
            return Err("保险库状态异常或尚未就绪，禁止解锁；请先处理恢复状态".into());
        }

        *self.status.write().unwrap() = VaultStatus::Unlocking;

        let envelope = match self.envelope() {
            Some(env) => env,
            None => {
                let loaded = VaultEnvelope::load_from_dir(&self.data_dir)
                    .map_err(|e| {
                        *self.status.write().unwrap() = VaultStatus::Locked;
                        format!("加载元数据失败: {}", e)
                    })?
                    .ok_or_else(|| {
                        *self.status.write().unwrap() = VaultStatus::Locked;
                        "未找到保险库元数据文件".to_string()
                    })?;
                *self.envelope.write().unwrap() = Some(loaded.clone());
                loaded
            }
        };

        let master_key = match unwrap_master_key_with_password(&envelope, password) {
            Ok(key) => key,
            Err(e) => {
                *self.status.write().unwrap() = VaultStatus::Locked;
                return Err(format!("口令错误或无法解密主密钥: {}", e));
            }
        };

        let mut conn = match open_encrypted_database(&self.db_path(), master_key.as_bytes(), false)
        {
            Ok(c) => c,
            Err(e) => {
                *self.status.write().unwrap() = VaultStatus::Locked;
                return Err(format!("打开加密数据库失败: {}", e));
            }
        };

        if let Err(e) = run_migrations(&mut conn) {
            drop(conn);
            let message = format!("数据库结构升级失败，已关闭连接并保留数据: {e}");
            *self.status.write().unwrap() = VaultStatus::Error(message.clone());
            return Err(message);
        }

        self.set_unlocked_at_generation(conn, master_key, envelope, request_generation)?;
        Ok(())
    }

    /// Unlocks the vault using the recovery key
    pub fn unlock_with_recovery_key(&self, recovery_key: &str) -> Result<(), String> {
        self.unlock_with_recovery_key_at_generation(recovery_key, self.request_generation()?)
    }

    pub(crate) fn unlock_with_recovery_key_at_generation(
        &self,
        recovery_key: &str,
        request_generation: u64,
    ) -> Result<(), String> {
        let _lifecycle = self
            .lifecycle
            .lock()
            .map_err(|_| "保险库生命周期锁不可用")?;
        self.check_generation(request_generation)?;
        let current_status = self.status();
        if current_status == VaultStatus::Unlocked {
            return Ok(());
        }
        if current_status != VaultStatus::Locked {
            return Err("保险库状态异常或尚未就绪，禁止解锁；请先处理恢复状态".into());
        }

        *self.status.write().unwrap() = VaultStatus::Unlocking;

        let envelope = match self.envelope() {
            Some(env) => env,
            None => {
                let loaded = VaultEnvelope::load_from_dir(&self.data_dir)
                    .map_err(|e| {
                        *self.status.write().unwrap() = VaultStatus::Locked;
                        format!("加载元数据失败: {}", e)
                    })?
                    .ok_or_else(|| {
                        *self.status.write().unwrap() = VaultStatus::Locked;
                        "未找到保险库元数据文件".to_string()
                    })?;
                *self.envelope.write().unwrap() = Some(loaded.clone());
                loaded
            }
        };

        let master_key = match unwrap_master_key_with_recovery(&envelope, recovery_key) {
            Ok(key) => key,
            Err(e) => {
                *self.status.write().unwrap() = VaultStatus::Locked;
                return Err(format!("恢复密钥无效或无法解密主密钥: {}", e));
            }
        };

        let mut conn = match open_encrypted_database(&self.db_path(), master_key.as_bytes(), false)
        {
            Ok(c) => c,
            Err(e) => {
                *self.status.write().unwrap() = VaultStatus::Locked;
                return Err(format!("打开加密数据库失败: {}", e));
            }
        };

        if let Err(e) = run_migrations(&mut conn) {
            drop(conn);
            let message = format!("数据库结构升级失败，已关闭连接并保留数据: {e}");
            *self.status.write().unwrap() = VaultStatus::Error(message.clone());
            return Err(message);
        }

        self.set_unlocked_at_generation(conn, master_key, envelope, request_generation)?;
        Ok(())
    }

    /// Re-wraps the master key with a new password without full database re-encryption
    pub fn change_password(&self, old_password: &str, new_password: &str) -> Result<(), String> {
        let _lifecycle = self
            .lifecycle
            .lock()
            .map_err(|_| "保险库生命周期锁不可用")?;
        let envelope = self
            .envelope()
            .or_else(|| VaultEnvelope::load_from_dir(&self.data_dir).ok().flatten())
            .ok_or_else(|| "未找到保险库元数据".to_string())?;

        let master_key = unwrap_master_key_with_password(&envelope, old_password)
            .map_err(|e| format!("原口令校验失败: {}", e))?;

        let mut updated_envelope = envelope.clone();
        change_envelope_password(
            &mut updated_envelope,
            &master_key,
            new_password,
            Utc::now().timestamp_millis(),
        )
        .map_err(|e| format!("更新口令失败: {}", e))?;

        updated_envelope
            .save_atomic(&self.data_dir)
            .map_err(|e| format!("保存新保险库元数据失败: {}", e))?;

        *self.envelope.write().unwrap() = Some(updated_envelope);
        info!("保险库主加密口令已成功更新");
        Ok(())
    }

    /// Safely migrates legacy plaintext SQLite to SQLCipher encrypted vault
    pub fn migrate_legacy_plaintext(&self, password: &str) -> Result<String, String> {
        self.migrate_legacy_plaintext_at_generation(password, self.request_generation()?)
    }

    pub(crate) fn migrate_legacy_plaintext_at_generation(
        &self,
        password: &str,
        request_generation: u64,
    ) -> Result<String, String> {
        let _lifecycle = self
            .lifecycle
            .lock()
            .map_err(|_| "保险库生命周期锁不可用")?;
        self.check_generation(request_generation)?;
        let current_status = self.status();
        if current_status != VaultStatus::LegacyPlaintext {
            return Err(format!("当前状态不是明文数据库状态 ({:?})", current_status));
        }

        *self.status.write().unwrap() = VaultStatus::Migrating;

        let (envelope, recovery_key_str) =
            match migrate_plaintext_to_sqlcipher(&self.data_dir, password) {
                Ok(res) => res,
                Err(e) => {
                    self.refresh_state();
                    return Err(format!("明文数据库加密迁移失败: {}", e));
                }
            };

        let master_key = match unwrap_master_key_with_password(&envelope, password) {
            Ok(key) => key,
            Err(e) => {
                self.refresh_state();
                return Err(format!("迁移后解密主密钥失败: {}", e));
            }
        };

        let conn = match open_encrypted_database(&self.db_path(), master_key.as_bytes(), false) {
            Ok(c) => c,
            Err(e) => {
                self.refresh_state();
                return Err(format!("打开迁移后的加密数据库失败: {}", e));
            }
        };

        self.set_unlocked_at_generation(conn, master_key, envelope, request_generation)?;
        info!("明文数据库安全迁移完成并成功解锁");
        Ok(recovery_key_str)
    }

    fn load_or_cached_envelope(&self) -> Result<VaultEnvelope, String> {
        if let Some(env) = self.envelope() {
            return Ok(env);
        }
        let loaded = VaultEnvelope::load_from_dir(&self.data_dir)
            .map_err(|e| format!("加载元数据失败: {}", e))?
            .ok_or_else(|| "未找到保险库元数据文件".to_string())?;
        *self.envelope.write().unwrap() = Some(loaded.clone());
        Ok(loaded)
    }

    /// Initializes or resumes passive OS-protected encryption at startup without prompting for a vault passphrase.
    /// Legacy passphrase-only vaults remain `Locked` until unlocked once, and vaults with an active UI PIN stay
    /// `Locked` until the user verifies their PIN.
    pub fn ensure_passive_vault_unlocked(&self) -> Result<(), String> {
        let request_generation = self.request_generation()?;
        let _lifecycle = self
            .lifecycle
            .lock()
            .map_err(|_| "保险库生命周期锁不可用")?;
        self.check_generation(request_generation)?;

        match self.status() {
            VaultStatus::Unlocked => Ok(()),
            VaultStatus::Unconfigured => {
                *self.status.write().unwrap() = VaultStatus::Unlocking;
                let (envelope, master_key) = match create_passive_vault_database(&self.data_dir) {
                    Ok(pair) => pair,
                    Err(e) => {
                        self.refresh_state();
                        return Err(format!("自动初始化加密保险库失败: {}", e));
                    }
                };
                let mut conn =
                    match open_encrypted_database(&self.db_path(), master_key.as_bytes(), false) {
                        Ok(c) => c,
                        Err(e) => {
                            let msg = format!("打开自动初始化加密数据库失败: {}", e);
                            *self.envelope.write().unwrap() = Some(envelope);
                            *self.status.write().unwrap() = VaultStatus::Error(msg.clone());
                            return Err(msg);
                        }
                    };
                if let Err(e) = run_migrations(&mut conn) {
                    drop(conn);
                    let msg = format!("数据库结构初始化失败: {}", e);
                    *self.envelope.write().unwrap() = Some(envelope);
                    *self.status.write().unwrap() = VaultStatus::Error(msg.clone());
                    return Err(msg);
                }
                self.set_unlocked_at_generation(conn, master_key, envelope, request_generation)?;
                info!("已启用后台静默数据库加密并自动解锁");
                Ok(())
            }
            VaultStatus::LegacyPlaintext => {
                *self.status.write().unwrap() = VaultStatus::Unlocking;
                let (envelope, master_key) =
                    match migrate_plaintext_to_sqlcipher_passive(&self.data_dir) {
                        Ok(pair) => pair,
                        Err(e) => {
                            self.refresh_state();
                            return Err(format!("后台迁移明文数据库失败: {}", e));
                        }
                    };
                let mut conn =
                    match open_encrypted_database(&self.db_path(), master_key.as_bytes(), false) {
                        Ok(c) => c,
                        Err(e) => {
                            let msg = format!("打开迁移后加密数据库失败: {}", e);
                            *self.envelope.write().unwrap() = Some(envelope);
                            *self.status.write().unwrap() = VaultStatus::Error(msg.clone());
                            return Err(msg);
                        }
                    };
                if let Err(e) = run_migrations(&mut conn) {
                    drop(conn);
                    let msg = format!("迁移后数据库结构升级失败，已关闭连接并保留数据: {}", e);
                    *self.envelope.write().unwrap() = Some(envelope);
                    *self.status.write().unwrap() = VaultStatus::Error(msg.clone());
                    return Err(msg);
                }
                self.set_unlocked_at_generation(conn, master_key, envelope, request_generation)?;
                info!("旧明文数据库已完成静默加密迁移并解锁");
                Ok(())
            }
            VaultStatus::Locked => {
                let envelope = self.load_or_cached_envelope()?;
                if !envelope.has_dpapi_wrap() {
                    // Legacy passphrase-only vault: keep Locked so user can unlock once and auto-enroll DPAPI.
                    return Ok(());
                }
                *self.status.write().unwrap() = VaultStatus::Unlocking;
                let master_key = match unwrap_master_key_with_dpapi(&envelope) {
                    Ok(key) => key,
                    Err(e) => {
                        *self.status.write().unwrap() = VaultStatus::Locked;
                        return Err(format!("使用本机密钥自动解封失败: {}", e));
                    }
                };
                let mut conn =
                    match open_encrypted_database(&self.db_path(), master_key.as_bytes(), false) {
                        Ok(c) => c,
                        Err(e) => {
                            *self.status.write().unwrap() = VaultStatus::Locked;
                            return Err(format!("打开加密数据库失败: {}", e));
                        }
                    };
                if let Err(e) = run_migrations(&mut conn) {
                    drop(conn);
                    let msg = format!("数据库结构升级失败，已关闭连接并保留数据: {}", e);
                    *self.status.write().unwrap() = VaultStatus::Error(msg.clone());
                    return Err(msg);
                }
                let pin_enabled = conn
                    .query_row(
                        "SELECT pin_enabled FROM app_security WHERE id = 1",
                        [],
                        |r| r.get::<_, i64>(0),
                    )
                    .map(|v| v != 0)
                    .unwrap_or(false);
                if pin_enabled {
                    drop(conn);
                    drop(master_key);
                    *self.envelope.write().unwrap() = Some(envelope);
                    *self.status.write().unwrap() = VaultStatus::Locked;
                    info!("已启用 4 位应用 PIN 锁，启动时保持保险库锁定等待 PIN 验证");
                    return Ok(());
                }
                self.set_unlocked_at_generation(conn, master_key, envelope, request_generation)?;
                Ok(())
            }
            VaultStatus::Error(err) => Err(err),
            VaultStatus::Locking | VaultStatus::Unlocking | VaultStatus::Migrating => {
                Err("保险库正在切换状态，请稍后再试".into())
            }
        }
    }

    /// Opens a lightweight, single-connection, low-cache SQLCipher session for headless background sync.
    /// Strictly requires an existing DPAPI-wrapped healthy vault with confirmed recovery key.
    pub(crate) fn open_headless_dpapi_session(&self) -> AppResult<HeadlessVaultSession> {
        let _lifecycle = self
            .lifecycle
            .lock()
            .map_err(|_| AppError::System("保险库生命周期锁不可用".into()))?;
        match self.status() {
            VaultStatus::Locked => {}
            VaultStatus::Unconfigured => {
                return Err(AppError::VaultLocked(
                    "保险库尚未初始化，后台任务已跳过".into(),
                ));
            }
            VaultStatus::LegacyPlaintext => {
                return Err(AppError::VaultLocked(
                    "保险库仍为旧版明文格式，请先打开主程序完成安全迁移".into(),
                ));
            }
            VaultStatus::Error(err) => {
                return Err(AppError::Security(format!(
                    "保险库处于异常现场状态，后台任务已安全跳过: {err}"
                )));
            }
            other => {
                return Err(AppError::VaultLocked(format!(
                    "保险库当前状态不可用于后台无头同步 ({:?})",
                    other
                )));
            }
        }

        let envelope = self.load_or_cached_envelope().map_err(AppError::Security)?;
        if !envelope.recovery_key_confirmed {
            return Err(AppError::VaultLocked(
                "保险库恢复密钥尚未确认，后台任务已跳过".into(),
            ));
        }
        if !envelope.has_dpapi_wrap() {
            return Err(AppError::VaultLocked(
                "保险库尚未绑定本机 DPAPI 自动密钥，请打开 Browsory 解锁一次以启用后台自动归档"
                    .into(),
            ));
        }

        let master_key = unwrap_master_key_with_dpapi(&envelope)
            .map_err(|e| AppError::Security(format!("本机 DPAPI 解封主密钥失败: {e}")))?;

        let mut conn = open_encrypted_database_with_cache(
            &self.db_path(),
            master_key.as_bytes(),
            false,
            -16_384,
        )?;
        run_migrations(&mut conn)?;

        Ok(HeadlessVaultSession {
            conn: Some(conn),
            _master_key: master_key,
        })
    }

    /// Attaches OS automatic key wrapping (`dpapi_wrap`) to the currently unlocked envelope if absent.
    pub fn enroll_dpapi_on_unlocked(&self) -> Result<bool, String> {
        let _lifecycle = self
            .lifecycle
            .lock()
            .map_err(|_| "保险库生命周期锁不可用")?;
        if self.status() != VaultStatus::Unlocked {
            return Err("保险库未解锁，无法配置本机自动密钥".into());
        }
        let mut envelope = self
            .envelope()
            .ok_or_else(|| "当前保险库元数据不可用".to_string())?;
        let master_key = self
            .master_key()
            .ok_or_else(|| "当前主密钥不可用".to_string())?;
        let now_ms = Utc::now().timestamp_millis();
        let changed = enroll_dpapi_wrap(&mut envelope, &master_key, now_ms)?;
        if !changed {
            return Ok(false);
        }
        envelope.save_atomic(&self.data_dir)?;
        *self.envelope.write().unwrap() = Some(envelope);
        info!("已为现有保险库补充本机 DPAPI 自动密钥封装");
        Ok(true)
    }

    pub fn resume_passive_vault_with_pin_check<F>(
        &self,
        check_pin: F,
    ) -> Result<PassiveResumeResult, String>
    where
        F: FnOnce(&Connection) -> Result<PassivePinCheck, String>,
    {
        self.resume_passive_vault_at_generation(check_pin, self.request_generation()?)
    }

    pub(crate) fn resume_passive_vault_at_generation<F>(
        &self,
        check_pin: F,
        request_generation: u64,
    ) -> Result<PassiveResumeResult, String>
    where
        F: FnOnce(&Connection) -> Result<PassivePinCheck, String>,
    {
        let _lifecycle = self
            .lifecycle
            .lock()
            .map_err(|_| "保险库生命周期锁不可用")?;
        self.check_generation(request_generation)?;
        let current_status = self.status();
        if current_status == VaultStatus::Unlocked {
            return Ok(PassiveResumeResult {
                unlocked: true,
                pin_required: false,
                is_locked_out: false,
                lockout_remaining_secs: 0,
                error_message: None,
            });
        }
        if current_status != VaultStatus::Locked {
            return Err("保险库状态异常或尚未就绪，禁止解锁；请先处理恢复状态".into());
        }

        *self.status.write().unwrap() = VaultStatus::Unlocking;
        let envelope = match self.load_or_cached_envelope() {
            Ok(env) => env,
            Err(e) => {
                *self.status.write().unwrap() = VaultStatus::Locked;
                return Err(e);
            }
        };
        if !envelope.has_dpapi_wrap() {
            *self.status.write().unwrap() = VaultStatus::Locked;
            return Err("当前保险库未绑定本机自动密钥，请使用原口令或恢复密钥解锁".into());
        }

        let master_key = match unwrap_master_key_with_dpapi(&envelope) {
            Ok(key) => key,
            Err(e) => {
                *self.status.write().unwrap() = VaultStatus::Locked;
                return Err(format!("无法使用本机密钥自动解封保险库: {}", e));
            }
        };

        let mut conn = match open_encrypted_database(&self.db_path(), master_key.as_bytes(), false)
        {
            Ok(c) => c,
            Err(e) => {
                *self.status.write().unwrap() = VaultStatus::Locked;
                return Err(format!("打开加密数据库失败: {}", e));
            }
        };

        if let Err(e) = run_migrations(&mut conn) {
            drop(conn);
            let message = format!("数据库结构升级失败，已关闭连接并保留数据: {e}");
            *self.status.write().unwrap() = VaultStatus::Error(message.clone());
            return Err(message);
        }

        match check_pin(&conn) {
            Ok(PassivePinCheck::AllowUnlock) => {
                self.set_unlocked_at_generation(conn, master_key, envelope, request_generation)?;
                Ok(PassiveResumeResult {
                    unlocked: true,
                    pin_required: false,
                    is_locked_out: false,
                    lockout_remaining_secs: 0,
                    error_message: None,
                })
            }
            Ok(PassivePinCheck::PinRequired) => {
                drop(conn);
                drop(master_key);
                *self.envelope.write().unwrap() = Some(envelope);
                *self.status.write().unwrap() = VaultStatus::Locked;
                Ok(PassiveResumeResult {
                    unlocked: false,
                    pin_required: true,
                    is_locked_out: false,
                    lockout_remaining_secs: 0,
                    error_message: None,
                })
            }
            Ok(PassivePinCheck::InvalidPin {
                is_locked_out,
                lockout_remaining_secs,
                message,
            }) => {
                drop(conn);
                drop(master_key);
                *self.envelope.write().unwrap() = Some(envelope);
                *self.status.write().unwrap() = VaultStatus::Locked;
                Ok(PassiveResumeResult {
                    unlocked: false,
                    pin_required: true,
                    is_locked_out,
                    lockout_remaining_secs,
                    error_message: Some(message),
                })
            }
            Ok(PassivePinCheck::LockedOut {
                remaining_secs,
                message,
            }) => {
                drop(conn);
                drop(master_key);
                *self.envelope.write().unwrap() = Some(envelope);
                *self.status.write().unwrap() = VaultStatus::Locked;
                Ok(PassiveResumeResult {
                    unlocked: false,
                    pin_required: true,
                    is_locked_out: true,
                    lockout_remaining_secs: remaining_secs,
                    error_message: message,
                })
            }
            Err(e) => {
                drop(conn);
                drop(master_key);
                *self.envelope.write().unwrap() = Some(envelope);
                *self.status.write().unwrap() = VaultStatus::Locked;
                Err(e)
            }
        }
    }

    /// Revoke first, cancel tracked futures, drain work, and physically close.
    pub fn lock(&self) -> Result<(), String> {
        let completion_token = self
            .revocation
            .fetch_add(2, Ordering::AcqRel)
            .wrapping_add(2);
        let _lifecycle = self
            .lifecycle
            .lock()
            .map_err(|_| "保险库生命周期锁不可用")?;
        if matches!(self.status(), VaultStatus::Error(_))
            && self.active_conn.read().unwrap().is_none()
        {
            return Err("保险库处于错误状态，已保留现场，禁止用锁定覆盖错误状态".into());
        }
        let result = self
            .close_active_session()
            .map_err(|error| error.to_string());
        if result.is_ok() {
            // Only this lock owner can acknowledge this exact request. Restore
            // closes never acknowledge it; newer requests make this CAS fail.
            let _ = self.revocation.compare_exchange(
                completion_token,
                completion_token & !1,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
        result
    }

    fn close_active_session(&self) -> AppResult<()> {
        let session = self.active_conn.write().unwrap().take();
        if let Some(session) = &session {
            session.revoke();
        }
        *self.status.write().unwrap() = VaultStatus::Locking;
        let result = session.map(|session| session.close()).unwrap_or(Ok(()));
        self.master_key.write().unwrap().take();
        if let Err(error) = &result {
            *self.status.write().unwrap() =
                VaultStatus::Error(format!("锁定时关闭数据库失败: {}", error));
        } else {
            *self.status.write().unwrap() = if self.envelope().is_some() {
                VaultStatus::Locked
            } else {
                VaultStatus::Unconfigured
            };
        }
        result
    }

    /// Promote a validated restore into a serialized replacement. Never wait
    /// for lifecycle while registered: a concurrent lock may be draining us.
    pub fn begin_replacement(&self, operation: VaultOperation) -> AppResult<VaultReplacement<'_>> {
        let lifecycle = self
            .lifecycle
            .try_lock()
            .map_err(|_| AppError::VaultLocked("保险库正在锁定或更改，恢复已取消".into()))?;
        operation.check()?;
        let current = self
            .active_conn
            .read()
            .unwrap()
            .clone()
            .ok_or_else(|| AppError::VaultLocked("恢复会话已结束".into()))?;
        if !operation.belongs_to(&current) {
            return Err(AppError::VaultLocked("恢复会话已经过期".into()));
        }
        drop(operation);
        self.close_active_session()?;
        Ok(VaultReplacement {
            manager: self,
            _lifecycle: lifecycle,
            installed: false,
        })
    }
}

/// Holds exclusive lifecycle ownership through file replacement and re-opening.
pub struct VaultReplacement<'a> {
    manager: &'a VaultManager,
    _lifecycle: MutexGuard<'a, ()>,
    installed: bool,
}

impl VaultReplacement<'_> {
    pub fn finish_locked(mut self, envelope: VaultEnvelope) {
        *self.manager.envelope.write().unwrap() = Some(envelope);
        self.manager.master_key.write().unwrap().take();
        *self.manager.status.write().unwrap() = VaultStatus::Locked;
        self.installed = true;
    }

    pub fn install(
        mut self,
        conn: Connection,
        key: MasterKey,
        envelope: VaultEnvelope,
    ) -> Result<(), String> {
        self.manager.set_unlocked(conn, key, envelope)?;
        self.installed = true;
        Ok(())
    }
}

impl Drop for VaultReplacement<'_> {
    fn drop(&mut self) {
        // File installation may have succeeded even if reopening failed. Do
        // not keep the old envelope cached against the new on-disk database.
        if !self.installed {
            self.manager.refresh_state();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use uuid::Uuid;

    #[test]
    fn restore_close_cannot_acknowledge_a_newer_power_lock_request() {
        use std::sync::mpsc;
        use std::time::{Duration, Instant};
        let root = std::env::temp_dir().join(format!("browsory_restore_power_{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let manager = Arc::new(VaultManager::unlocked_fixture(root.clone()));
        let envelope = manager.envelope().unwrap();
        let old_generation = manager.request_generation().unwrap();
        let restore_operation = manager.begin_operation().unwrap();
        let draining_operation = restore_operation.fork().unwrap();
        let restoring = manager.clone();
        let (closed_tx, closed_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let restore = std::thread::spawn(move || {
            let replacement = restoring.begin_replacement(restore_operation).unwrap();
            closed_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            replacement.finish_locked(envelope);
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while manager.status() != VaultStatus::Locking {
            assert!(Instant::now() < deadline, "restore never entered drain");
            std::thread::yield_now();
        }
        // The restore owns lifecycle and cannot finish until this child drops.
        manager.revoke_active_session();
        drop(draining_operation);
        closed_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(
            manager.request_generation().is_err(),
            "restore consumed a power lock request"
        );
        let unlocking = manager.clone();
        let queued = std::thread::spawn(move || {
            unlocking
                .unlock_with_password_at_generation("Synthetic-fixture-password", old_generation)
        });
        let closing = manager.clone();
        let power_close = std::thread::spawn(move || closing.lock());
        release_tx.send(()).unwrap();
        restore.join().unwrap();
        power_close.join().unwrap().unwrap();
        assert!(queued.join().unwrap().is_err());
        assert_eq!(manager.status(), VaultStatus::Locked);
        assert!(manager.connection().is_err());
        // A newer explicit unlock is allowed only after the lock owner finishes.
        assert!(manager.request_generation().is_ok());
        drop(manager);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn queued_unlock_cannot_reopen_after_a_platform_lock() {
        let root = std::env::temp_dir().join(format!("browsory_queued_unlock_{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let manager = Arc::new(VaultManager::unlocked_fixture(root.clone()));
        let request_generation = manager.request_generation().unwrap();
        let held_lifecycle = manager.lifecycle.lock().unwrap();
        let unlock_manager = manager.clone();
        let queued = std::thread::spawn(move || {
            unlock_manager.unlock_with_password_at_generation(
                "Synthetic-fixture-password",
                request_generation,
            )
        });
        manager.revoke_active_session();
        drop(held_lifecycle);
        manager.lock().unwrap();
        assert!(queued.join().unwrap().is_err());
        assert_eq!(manager.status(), VaultStatus::Locked);
        assert!(manager.connection().is_err());
        drop(manager);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn platform_revocation_does_not_wait_for_lifecycle_and_blocks_session_install() {
        let root = std::env::temp_dir().join(format!("browsory_power_revoke_{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let manager = VaultManager::unlocked_fixture(root.clone());
        let key = manager.master_key().unwrap();
        let envelope = manager.envelope().unwrap();
        let operation = manager.begin_operation().unwrap();
        let lifecycle = manager.lifecycle.lock().unwrap();
        // A synchronous power callback can revoke while another lifecycle
        // transition owns the mutex. It neither drains nor reacquires it.
        manager.revoke_active_session();
        assert!(operation.check().is_err());
        assert_eq!(manager.status(), VaultStatus::Locking);
        assert!(manager.connection().is_err());
        drop(operation);
        drop(lifecycle);
        manager.lock().unwrap();
        manager.revoke_active_session();
        let conn =
            open_encrypted_database(&root.join("archive.db"), key.as_bytes(), false).unwrap();
        assert!(manager.set_unlocked(conn, key, envelope).is_err());
        assert!(manager.connection().is_err());
        manager.lock().unwrap();
        assert_eq!(manager.status(), VaultStatus::Locked);
        drop(manager);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn both_unlock_methods_fail_closed_on_schema_errors_and_future_versions() {
        use super::super::envelope::{create_vault_envelope, KdfParameters, MIN_MEM_COST_KB};
        let (mut envelope, recovery, key) = create_vault_envelope(
            "Schema-upgrade-test-password",
            Some(KdfParameters {
                mem_cost_kb: MIN_MEM_COST_KB,
                time_cost: 1,
                parallelism: 1,
            }),
            0,
        )
        .unwrap();
        envelope.recovery_key_confirmed = true;
        for use_recovery in [false, true] {
            for fault in ["trigger", "future", "invalid_marker_schema"] {
                let root =
                    std::env::temp_dir().join(format!("browsory_schema_fail_{}", Uuid::new_v4()));
                fs::create_dir_all(&root).unwrap();
                envelope.save_atomic(&root).unwrap();
                let mut conn =
                    open_encrypted_database(&root.join("archive.db"), key.as_bytes(), true)
                        .unwrap();
                run_migrations(&mut conn).unwrap();
                conn.execute_batch("CREATE TABLE preserved_data(value TEXT); INSERT INTO preserved_data VALUES('keep original');").unwrap();
                match fault {
                    "trigger" => conn.execute_batch(&format!("DELETE FROM schema_migrations WHERE version={}; CREATE TRIGGER refuse_upgrade BEFORE INSERT ON schema_migrations BEGIN SELECT RAISE(ABORT,'synthetic upgrade failure'); END;", crate::database::migrations::latest_schema_version())).unwrap(),
                    "future" => { conn.execute("INSERT INTO schema_migrations VALUES(999,'future',0)", []).unwrap(); },
                    _ => conn.execute_batch("DROP TABLE schema_migrations; CREATE TABLE schema_migrations(version TEXT); INSERT INTO schema_migrations VALUES('invalid-version');").unwrap(),
                }
                drop(conn);
                let vault = VaultManager::new(root.clone());
                let result = if use_recovery {
                    vault.unlock_with_recovery_key(&recovery)
                } else {
                    vault.unlock_with_password("Schema-upgrade-test-password")
                };
                assert!(result.is_err(), "{fault}, recovery={use_recovery}");
                assert!(matches!(vault.status(), VaultStatus::Error(_)));
                assert!(vault.master_key().is_none());
                assert!(vault.connection().is_err());
                assert!(vault.lock().is_err());
                drop(vault);
                let conn = open_encrypted_database(&root.join("archive.db"), key.as_bytes(), false)
                    .unwrap();
                assert_eq!(
                    conn.query_row("SELECT value FROM preserved_data", [], |r| r
                        .get::<_, String>(0))
                        .unwrap(),
                    "keep original"
                );
                if fault == "trigger" {
                    assert_eq!(
                        conn.query_row(
                            "SELECT count(*) FROM schema_migrations WHERE version=?1",
                            [crate::database::migrations::latest_schema_version()],
                            |r| r.get::<_, i64>(0)
                        )
                        .unwrap(),
                        0
                    );
                }
                drop(conn);
                fs::remove_dir_all(root).unwrap();
            }
        }
    }

    #[test]
    fn corrupt_restore_state_cannot_be_bypassed_by_unlock_or_lock_commands() {
        let root =
            std::env::temp_dir().join(format!("browsory_restore_error_guard_{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let vault = VaultManager::unlocked_fixture(root.clone());
        vault.lock().unwrap();
        drop(vault);
        fs::write(root.join("restore.state.json"), b"{corrupt").unwrap();
        let vault = VaultManager::new(root.clone());
        let before = fs::read(root.join("archive.db")).unwrap();
        let error = vault.status();
        assert!(matches!(error, VaultStatus::Error(_)));
        assert!(vault
            .unlock_with_password("Synthetic-fixture-password")
            .is_err());
        assert!(vault
            .unlock_with_recovery_key("must reject before key derivation")
            .is_err());
        assert!(vault.lock().is_err());
        assert_eq!(vault.status(), error);
        assert!(vault.connection().is_err());
        assert_eq!(fs::read(root.join("archive.db")).unwrap(), before);
        assert_eq!(
            fs::read(root.join("restore.state.json")).unwrap(),
            b"{corrupt"
        );
        drop(vault);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn locking_drains_snapshot_work_and_releases_the_database_file() {
        use std::sync::mpsc;
        use std::time::{Duration, Instant};
        let root = std::env::temp_dir().join(format!("browsory_snapshot_drain_{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("browser-source.db");
        {
            let conn = Connection::open(&source).unwrap();
            conn.execute_batch("CREATE TABLE t(value TEXT); INSERT INTO t VALUES('synthetic');")
                .unwrap();
        }
        let vault = Arc::new(VaultManager::unlocked_fixture(root.clone()));
        let retained = vault.connection().unwrap();
        let operation = vault.begin_operation().unwrap();
        let temporary = root.join("temp");
        let snapshot_released = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_released = snapshot_released.clone();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let snapshot =
                crate::history::snapshot::create_history_snapshot(&source, &temporary).unwrap();
            assert_eq!(
                snapshot
                    .connection()
                    .query_row("SELECT value FROM t", [], |r| r.get::<_, String>(0))
                    .unwrap(),
                "synthetic"
            );
            ready_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            assert!(operation.check().is_err());
            assert!(
                operation.connection().is_err(),
                "cancelled import must not commit"
            );
            drop(snapshot);
            worker_released.store(true, std::sync::atomic::Ordering::Release);
            drop(operation);
        });
        ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let closing = vault.clone();
        let (done_tx, done_rx) = mpsc::channel();
        let locker = std::thread::spawn(move || {
            closing.lock().unwrap();
            done_tx.send(()).unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while vault.status() != VaultStatus::Locking {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(retained.lock().is_err());
        assert!(done_rx.try_recv().is_err());
        assert!(!snapshot_released.load(std::sync::atomic::Ordering::Acquire));
        assert!(
            !root.join("temp").exists(),
            "memory snapshot never creates a disk copy"
        );
        release_tx.send(()).unwrap();
        done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        worker.join().unwrap();
        locker.join().unwrap();
        assert!(
            snapshot_released.load(std::sync::atomic::Ordering::Acquire),
            "lock must wait for snapshot release"
        );
        assert_eq!(vault.status(), VaultStatus::Locked);
        assert!(vault.master_key().is_none());
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            let exclusive = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .share_mode(0)
                .open(vault.db_path())
                .unwrap();
            drop(exclusive);
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn restore_promotion_does_not_deadlock_or_resurrect_after_a_lock() {
        use std::sync::mpsc;
        use std::time::{Duration, Instant};
        let root = std::env::temp_dir().join(format!("browsory_restore_lock_{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let vault = Arc::new(VaultManager::unlocked_fixture(root.clone()));
        let operation = vault.begin_operation().unwrap();
        let closing = vault.clone();
        let (done_tx, done_rx) = mpsc::channel();
        let locker = std::thread::spawn(move || {
            closing.lock().unwrap();
            done_tx.send(()).unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while vault.status() != VaultStatus::Locking {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        // Promotion must try-lock lifecycle, drop its registration on failure,
        // and let the lock drain. Waiting for lifecycle here would deadlock.
        assert!(vault.begin_replacement(operation).is_err());
        done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        locker.join().unwrap();
        assert_eq!(vault.status(), VaultStatus::Locked);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn lock_waits_for_restore_install_and_then_closes_the_replacement() {
        use std::sync::mpsc;
        use std::time::Duration;
        let root =
            std::env::temp_dir().join(format!("browsory_restore_install_{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let vault = Arc::new(VaultManager::unlocked_fixture(root.clone()));
        let key = vault.master_key().unwrap();
        let envelope = vault.envelope().unwrap();
        let operation = vault.begin_operation().unwrap();
        let replacement = vault.begin_replacement(operation).unwrap();
        let closing = vault.clone();
        let (started_tx, started_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let locker = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            closing.lock().unwrap();
            done_tx.send(()).unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(done_rx.try_recv().is_err());
        let conn = open_encrypted_database(&vault.db_path(), key.as_bytes(), false).unwrap();
        replacement.install(conn, key, envelope).unwrap();
        done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        locker.join().unwrap();
        assert_eq!(vault.status(), VaultStatus::Locked);
        assert!(vault.connection().is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unconfirmed_recovery_delivery_survives_restart_without_plaintext_key_storage() {
        let root =
            std::env::temp_dir().join(format!("browsory_recovery_delivery_{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let password = "Delivery-interruption-password";
        let vault = VaultManager::new(root.clone());
        let lost_key = vault.create_vault(password).unwrap();
        assert!(!vault.envelope().unwrap().recovery_key_confirmed);
        vault.lock().unwrap();
        drop(vault);
        let vault = VaultManager::new(root.clone());
        assert_eq!(vault.status(), VaultStatus::Locked);
        assert!(!vault.envelope().unwrap().recovery_key_confirmed);
        assert!(vault.regenerate_unconfirmed_recovery_key(password).is_err());
        vault.unlock_with_password(password).unwrap();
        let old_envelope = vault.envelope().unwrap();
        assert!(vault
            .regenerate_unconfirmed_recovery_key("wrong password")
            .is_err());
        assert_eq!(vault.envelope().unwrap(), old_envelope);
        let before =
            crate::security::backup::compute_file_sha256(&root.join("archive.db")).unwrap();
        let replacement = vault.regenerate_unconfirmed_recovery_key(password).unwrap();
        let after = crate::security::backup::compute_file_sha256(&root.join("archive.db")).unwrap();
        assert_eq!(
            before, after,
            "reissuing a recovery wrap must not re-encrypt the DB"
        );
        assert!(unwrap_master_key_with_recovery(&vault.envelope().unwrap(), &lost_key).is_err());
        assert!(vault.confirm_recovery_key(&lost_key).is_err());
        assert!(vault.confirm_recovery_key(&replacement).unwrap());
        assert!(
            !vault.confirm_recovery_key(&replacement).unwrap(),
            "confirmation retry must not restart lifecycle tasks"
        );
        assert!(vault.envelope().unwrap().recovery_key_confirmed);
        assert!(
            VaultEnvelope::load_from_dir(&root)
                .unwrap()
                .unwrap()
                .recovery_key_confirmed
        );
        let persisted = fs::read_to_string(vault.envelope_path()).unwrap();
        assert!(!persisted.contains(&replacement));
        assert!(!persisted.contains(password));
        vault.lock().unwrap();
        vault.unlock_with_recovery_key(&replacement).unwrap();
        vault.lock().unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unlocked_session_can_reissue_an_unconfirmed_recovery_key_without_passphrase() {
        let root = std::env::temp_dir().join(format!(
            "browsory_session_recovery_delivery_{}",
            Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let vault = VaultManager::new(root.clone());
        let lost_key = vault
            .create_vault("Session-recovery-test-password")
            .unwrap();
        let replacement = vault
            .regenerate_unconfirmed_recovery_key_from_session()
            .unwrap();
        assert_ne!(replacement, lost_key);
        assert!(vault.confirm_recovery_key(&lost_key).is_err());
        assert!(vault.confirm_recovery_key(&replacement).unwrap());
        vault.lock().unwrap();
        vault.unlock_with_recovery_key(&replacement).unwrap();
        vault.lock().unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn test_vault_manager_full_lifecycle() {
        let temp_dir = std::env::temp_dir().join(format!("browsory_vault_test_{}", Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();

        let vault = VaultManager::new(temp_dir.clone());
        assert_eq!(vault.status(), VaultStatus::Unconfigured);
        assert!(!vault.is_unlocked());

        // 1. Guard check: with_conn fails on unconfigured
        let guard_res = vault.with_conn(|_| Ok(()));
        assert!(
            guard_res.is_err(),
            "Access when unconfigured must be rejected"
        );

        // 2. Create vault
        let password = "PrimaryStrongVaultPassword!2026";
        let recovery_key = vault.create_vault(password).expect("Create vault");
        assert_eq!(vault.status(), VaultStatus::Unlocked);
        assert!(vault.is_unlocked());

        // 3. Unlocked access: perform writes & reads
        vault.with_conn(|conn| {
            conn.execute(
                "INSERT INTO urls (id, url, title, domain) VALUES (1, 'https://test.vault.browsory', 'Vault Test', 'browsory');",
                [],
            )?;
            let title: String = conn.query_row("SELECT title FROM urls WHERE id = 1;", [], |r| r.get(0))?;
            assert_eq!(title, "Vault Test");
            Ok(())
        }).expect("Execute query when unlocked");

        // 4. Lock vault
        vault.lock().expect("Lock vault");
        assert_eq!(vault.status(), VaultStatus::Locked);
        assert!(!vault.is_unlocked());

        // 5. Guard check: with_conn fails on locked
        let locked_guard_res = vault.with_conn(|_| Ok(()));
        assert!(
            locked_guard_res.is_err(),
            "Access when locked must be rejected with VaultLocked"
        );

        // 6. Wrong password unlock fails
        let wrong_res = vault.unlock_with_password("wrong-password-attempt");
        assert!(wrong_res.is_err(), "Wrong password must fail");
        assert_eq!(vault.status(), VaultStatus::Locked);

        // 7. Correct password unlock succeeds
        vault
            .unlock_with_password(password)
            .expect("Unlock with correct password");
        assert_eq!(vault.status(), VaultStatus::Unlocked);

        // Verify data intact
        vault
            .with_conn(|conn| {
                let title: String =
                    conn.query_row("SELECT title FROM urls WHERE id = 1;", [], |r| r.get(0))?;
                assert_eq!(title, "Vault Test");
                Ok(())
            })
            .expect("Read data after unlocking");

        // 8. Change password
        let new_password = "UpdatedNewPassword!2026";
        vault
            .change_password(password, new_password)
            .expect("Change password");

        // 9. Re-lock and unlock with new password
        vault.lock().expect("Lock vault again");
        let old_pwd_fail = vault.unlock_with_password(password);
        assert!(old_pwd_fail.is_err(), "Old password must no longer work");

        vault
            .unlock_with_password(new_password)
            .expect("Unlock with new password");
        assert_eq!(vault.status(), VaultStatus::Unlocked);

        // 10. Re-lock and unlock with recovery key
        vault.lock().expect("Lock before recovery key test");
        vault
            .unlock_with_recovery_key(&recovery_key)
            .expect("Unlock with recovery key");
        assert_eq!(vault.status(), VaultStatus::Unlocked);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[cfg(windows)]
    #[test]
    fn passive_dpapi_vault_lifecycle_supports_silent_start_and_pin_resume() {
        let root = std::env::temp_dir().join(format!("browsory_passive_vault_{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();

        // 1. Fresh directory auto-initializes encrypted DB & unlocks immediately.
        let vault = VaultManager::new(root.clone());
        assert_eq!(vault.status(), VaultStatus::Unconfigured);
        vault.ensure_passive_vault_unlocked().unwrap();
        assert_eq!(vault.status(), VaultStatus::Unlocked);
        assert!(vault.envelope().unwrap().has_dpapi_wrap());
        assert!(!vault.envelope().unwrap().recovery_key_confirmed);
        let recovery_key = vault
            .regenerate_unconfirmed_recovery_key_from_session()
            .unwrap();
        assert!(vault.confirm_recovery_key(&recovery_key).unwrap());
        assert!(vault.envelope().unwrap().recovery_key_confirmed);

        vault
            .with_conn(|conn| {
                conn.execute(
                    "INSERT INTO urls (id, url, title, domain) VALUES (1, 'https://passive.browsory', 'Passive Vault', 'browsory');",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        // 2. Relock and auto-resume when PIN is disabled.
        vault.lock().unwrap();
        assert_eq!(vault.status(), VaultStatus::Locked);
        vault.ensure_passive_vault_unlocked().unwrap();
        assert_eq!(vault.status(), VaultStatus::Unlocked);

        // 3. When PIN is enabled in DB, startup keeps the vault Locked until PIN verification succeeds.
        vault
            .with_conn(|conn| {
                conn.execute("UPDATE app_security SET pin_enabled = 1 WHERE id = 1", [])?;
                Ok(())
            })
            .unwrap();
        vault.lock().unwrap();
        vault.ensure_passive_vault_unlocked().unwrap();
        assert_eq!(vault.status(), VaultStatus::Locked);
        assert!(vault.connection().is_err());

        let need_pin = vault
            .resume_passive_vault_with_pin_check(|_| Ok(PassivePinCheck::PinRequired))
            .unwrap();
        assert!(!need_pin.unlocked);
        assert!(need_pin.pin_required);
        assert_eq!(vault.status(), VaultStatus::Locked);
        assert!(vault.connection().is_err());

        let allowed = vault
            .resume_passive_vault_with_pin_check(|conn| {
                let count: i64 = conn
                    .query_row("SELECT COUNT(*) FROM urls", [], |r| r.get(0))
                    .map_err(|e| e.to_string())?;
                assert_eq!(count, 1);
                Ok(PassivePinCheck::AllowUnlock)
            })
            .unwrap();
        assert!(allowed.unlocked);
        assert_eq!(vault.status(), VaultStatus::Unlocked);

        // 4. Legacy passphrase vault enrolls DPAPI once unlocked.
        let legacy_root =
            std::env::temp_dir().join(format!("browsory_legacy_enroll_{}", Uuid::new_v4()));
        fs::create_dir_all(&legacy_root).unwrap();
        let legacy_vault = VaultManager::new(legacy_root.clone());
        legacy_vault.create_vault("Legacy-passphrase-2026").unwrap();
        assert!(!legacy_vault.envelope().unwrap().has_dpapi_wrap());
        legacy_vault.lock().unwrap();
        // Locked legacy vault without DPAPI stays Locked across ensure_passive_vault_unlocked.
        legacy_vault.ensure_passive_vault_unlocked().unwrap();
        assert_eq!(legacy_vault.status(), VaultStatus::Locked);

        legacy_vault
            .unlock_with_password("Legacy-passphrase-2026")
            .unwrap();
        assert!(legacy_vault.enroll_dpapi_on_unlocked().unwrap());
        legacy_vault.lock().unwrap();
        legacy_vault.ensure_passive_vault_unlocked().unwrap();
        assert_eq!(legacy_vault.status(), VaultStatus::Unlocked);

        // 5. Headless DPAPI session opens locked DPAPI vault and rejects unconfigured/non-DPAPI vaults.
        vault.lock().unwrap();
        {
            let headless = vault.open_headless_dpapi_session().unwrap();
            let count: i64 = headless
                .connection()
                .query_row("SELECT COUNT(*) FROM urls", [], |r| r.get(0))
                .unwrap();
            assert_eq!(count, 1);
        }
        let unconfigured_root =
            std::env::temp_dir().join(format!("browsory_unconf_headless_{}", Uuid::new_v4()));
        fs::create_dir_all(&unconfigured_root).unwrap();
        let unconfigured_vault = VaultManager::new(unconfigured_root.clone());
        assert!(unconfigured_vault.open_headless_dpapi_session().is_err());

        drop(vault);
        drop(legacy_vault);
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(legacy_root);
        let _ = fs::remove_dir_all(unconfigured_root);
    }

    #[test]
    fn stale_generation_during_set_unlocked_restores_locked_status() {
        let root =
            std::env::temp_dir().join(format!("browsory_stale_gen_status_{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let password = "Stale-Gen-Status-Test-2026!";
        let vault = VaultManager::new(root.clone());
        vault.create_vault(password).unwrap();
        let envelope = vault.envelope().unwrap();
        vault.lock().unwrap();
        assert_eq!(vault.status(), VaultStatus::Locked);

        let stale_gen = vault.request_generation().unwrap();
        let master_key = unwrap_master_key_with_password(&envelope, password).unwrap();
        let conn = open_encrypted_database(&vault.db_path(), master_key.as_bytes(), false).unwrap();

        // Advance generation without leaving the pending bit set, then exercise set_unlocked_at_generation
        vault.lock().unwrap();
        *vault.status.write().unwrap() = VaultStatus::Unlocking;
        let err = vault
            .set_unlocked_at_generation(conn, master_key, envelope, stale_gen)
            .unwrap_err();
        assert!(err.contains("锁定请求"), "unexpected error: {err}");
        assert_eq!(vault.status(), VaultStatus::Locked);

        vault.unlock_with_password(password).unwrap();
        assert_eq!(vault.status(), VaultStatus::Unlocked);
        vault.lock().unwrap();
        drop(vault);
        let _ = fs::remove_dir_all(root);
    }
}
