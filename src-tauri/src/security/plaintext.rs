//! Explicit retirement of the single migration-retained plaintext database.
//! Never invoked on startup; never scans/deletes arbitrary files or free space.
use std::fs;
use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use zeroize::Zeroizing;

use super::backup::compute_file_sha256;
use super::envelope::{
    unwrap_master_key_with_dpapi, unwrap_master_key_with_password, MasterKey, VaultEnvelope,
};
use crate::database::encrypt_migration::{is_plain_sqlite_database, verify_export};
use crate::database::space::{protected_snapshot_requirement, require_available_space};
use crate::database::sqlcipher::{check_encrypted_integrity, open_encrypted_database};
use crate::error::{AppError, AppResult};
use crate::history::record_spool::CancelProbe;

const ORIGINAL: &str = "archive.db.plaintext.bak";
const CONFIRMATION: &str = "移除旧明文副本";
static RETIREMENT: Mutex<()> = Mutex::new(());

fn source_wal_size(path: &Path) -> AppResult<u64> {
    let mut name = path
        .file_name()
        .ok_or_else(|| refused("旧副本路径缺少文件名"))?
        .to_os_string();
    name.push("-wal");
    match fs::metadata(path.with_file_name(name)) {
        Ok(metadata) => Ok(metadata.len()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error.into()),
    }
}

#[derive(Serialize)]
pub struct PlaintextResidueInfo {
    pub legacy_backup_bytes: Option<u64>,
    pub blocked: bool,
}

fn refused(message: &str) -> AppError {
    AppError::Security(message.into())
}

fn plain_metadata(path: &Path) -> AppResult<Option<fs::Metadata>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(refused("旧副本或归档目录是重解析点，拒绝处理"));
        }
    }
    if metadata.file_type().is_symlink() {
        return Err(refused("旧副本或归档目录是链接，拒绝处理"));
    }
    Ok(Some(metadata))
}

fn check_recovery_material(directory: &Path) -> AppResult<()> {
    for name in [
        "migration.state.json",
        "restore.state.json",
        "vault.restore",
        "archive.staging.db",
        "vault.envelope.pending.json",
        "archive.db-wal.plaintext.bak",
        "archive.db-shm.plaintext.bak",
        "archive.db-journal.plaintext.bak",
        "archive.db.plaintext.bak-wal",
        "archive.db.plaintext.bak-shm",
        "archive.db.plaintext.bak-journal",
    ] {
        if match fs::symlink_metadata(directory.join(name)) {
            Ok(_) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(error.into()),
        } {
            return Err(refused(
                "存在迁移/恢复状态或旧库侧文件，请先处理；保留全部明文材料",
            ));
        }
    }
    Ok(())
}

pub fn inspect(directory: &Path) -> AppResult<PlaintextResidueInfo> {
    let metadata = plain_metadata(&directory.join(ORIGINAL))?;
    if metadata
        .as_ref()
        .is_some_and(|metadata| !metadata.is_file())
    {
        return Err(refused("旧明文副本不是普通文件"));
    }
    Ok(PlaintextResidueInfo {
        legacy_backup_bytes: metadata.map(|metadata| metadata.len()),
        blocked: check_recovery_material(directory).is_err(),
    })
}

/// User must type confirmation AND reauthenticate. A verified encrypted copy
/// is retained before unlinking the exact known original; no secure-erase claim.
pub fn retire<F>(
    directory: &Path,
    key: &MasterKey,
    envelope: &VaultEnvelope,
    password: &str,
    confirmation: &str,
    check_session: F,
) -> AppResult<String>
where
    F: Fn() -> AppResult<()>,
{
    retire_checked(
        directory,
        key,
        envelope,
        password,
        confirmation,
        (check_session, std::sync::Arc::new(|| false)),
    )
}

pub(crate) fn retire_checked<F: Fn() -> AppResult<()>>(
    directory: &Path,
    key: &MasterKey,
    envelope: &VaultEnvelope,
    password: &str,
    confirmation: &str,
    session: (F, CancelProbe),
) -> AppResult<String> {
    let (check_session, cancelled) = session;
    let _exclusive = RETIREMENT
        .try_lock()
        .map_err(|_| refused("旧副本归档正在进行，请稍后重试"))?;
    if confirmation != CONFIRMATION {
        return Err(refused("请输入“移除旧明文副本”确认本次操作"));
    }
    if !envelope.recovery_key_confirmed {
        return Err(refused("请先保存并确认恢复密钥"));
    }
    let authenticated = if envelope.has_dpapi_wrap() && password.trim().is_empty() {
        unwrap_master_key_with_dpapi(envelope).map_err(AppError::Security)?
    } else {
        unwrap_master_key_with_password(envelope, password).map_err(AppError::Security)?
    };
    if authenticated.as_bytes() != key.as_bytes() {
        return Err(refused("口令与当前保险库不匹配"));
    }
    check_session()?;
    check_recovery_material(directory)?;
    let original = directory.join(ORIGINAL);
    let info = plain_metadata(&original)?.ok_or_else(|| refused("未找到迁移保留的旧明文副本"))?;
    if !info.is_file() || !is_plain_sqlite_database(&original) {
        return Err(refused("旧副本不是普通明文 SQLite 文件，拒绝移除"));
    }
    let old_hash = compute_file_sha256(&original)?;
    // Export logical rows into an encrypted attachment without copying the
    // whole source database into a Rust/SQLite in-memory snapshot.
    let source_size = info
        .len()
        .checked_add(source_wal_size(&original)?)
        .ok_or_else(|| refused("旧明文副本大小溢出"))?;
    if source_size > super::backup::MAX_DATABASE_BYTES {
        return Err(refused("旧明文副本超过64GiB处理上限"));
    }
    require_available_space(directory, protected_snapshot_requirement(source_size)?)?;
    let mut source = Connection::open_with_flags(
        &original,
        // SQLCipher's export helper may use connection-local temporary state
        // and therefore uses a writable handle. No SQL source writes are
        // issued; final file hashing still detects source changes.
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    crate::database::sqlcipher::configure_private_connection(&source)?;
    source.busy_timeout(std::time::Duration::from_millis(100))?;
    source.pragma_update(None, "cache_size", -8192)?;
    source.pragma_update(None, "automatic_index", false)?;
    let _: u64 = source.query_row("PRAGMA mmap_size=0", [], |r| r.get(0))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(900);
    let probe = cancelled.clone();
    source.progress_handler(
        10_000,
        Some(move || probe() || std::time::Instant::now() >= deadline),
    )?;
    let integrity: String = source.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    if integrity != "ok" {
        return Err(refused("旧明文副本完整性检查失败，保留原件"));
    }
    let archive_root = directory.join("plaintext-retired");
    if crate::security::fs_boundary::resolve_scoped_path(directory, Path::new("plaintext-retired"))?
        .is_none()
    {
        fs::create_dir(&archive_root)?;
    }
    if !plain_metadata(&archive_root)?.is_some_and(|metadata| metadata.is_dir()) {
        return Err(refused("加密归档目录无效"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    let archive = archive_root.join(&id);
    fs::create_dir(&archive)?;
    // Failed partial output is encrypted and deliberately retained for review.
    let encrypted = archive.join("archive.db");
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&encrypted)?;
    let key_hex = Zeroizing::new(hex::encode(key.as_bytes()));
    let raw_key = Zeroizing::new(format!("x'{}'", key_hex.as_str()));
    source.execute(
        "ATTACH DATABASE ?1 AS encrypted KEY ?2",
        rusqlite::params![encrypted.to_string_lossy().as_ref(), raw_key.as_str()],
    )?;
    let user_version: i64 = source.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let application_id: i64 = source.pragma_query_value(None, "application_id", |r| r.get(0))?;
    {
        let tx = source.transaction()?;
        tx.query_row("SELECT sqlcipher_export('encrypted')", [], |_| Ok(()))?;
        tx.execute_batch(&format!(
            "PRAGMA encrypted.user_version={user_version}; PRAGMA encrypted.application_id={application_id};"
        ))?;
        tx.commit()?;
    }
    source.execute_batch("DETACH DATABASE encrypted")?;
    source.execute(
        "ATTACH DATABASE ?1 AS encrypted KEY ?2",
        rusqlite::params![encrypted.to_string_lossy().as_ref(), raw_key.as_str()],
    )?;
    source.execute_batch("BEGIN DEFERRED")?;
    verify_export(&source)?;
    source.execute_batch("ROLLBACK; DETACH DATABASE encrypted")?;
    drop(source);
    envelope.save_atomic(&archive).map_err(AppError::Security)?;
    {
        let mut reopened = open_encrypted_database(&encrypted, key.as_bytes(), false)?;
        let probe = cancelled.clone();
        reopened.progress_handler(
            10_000,
            Some(move || probe() || std::time::Instant::now() >= deadline),
        )?;
        if !check_encrypted_integrity(&reopened)? {
            return Err(refused("加密旧副本校验失败，保留原件"));
        }
        let busy: i64 = reopened.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))?;
        if busy != 0 {
            return Err(refused("加密旧副本 WAL 未完成合并，保留原件"));
        }
    }
    fs::OpenOptions::new()
        .write(true)
        .open(&encrypted)?
        .sync_all()?;
    let receipt = serde_json::json!({"format_version":1,"original_sha256":old_hash,
        "encrypted_sha256":compute_file_sha256(&encrypted)?,
        "envelope_sha256":compute_file_sha256(&archive.join("vault.envelope.json"))?});
    super::atomic_file::write_atomic(
        &archive.join("receipt.json"),
        &serde_json::to_vec_pretty(&receipt)?,
    )?;
    // Recheck after every expensive stage, before touching the original.
    check_session()?;
    check_recovery_material(directory)?;
    if !plain_metadata(&original)?.is_some_and(|metadata| metadata.is_file())
        || compute_file_sha256(&original)? != old_hash
    {
        return Err(refused("旧明文副本已变化，保留原件与已生成的加密归档"));
    }
    fs::remove_file(&original)?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::vault::VaultManager;
    static SERIAL: Mutex<()> = Mutex::new(());

    fn fixture() -> (std::path::PathBuf, VaultManager) {
        let root = std::env::temp_dir().join(format!(
            "browsory_retire_plaintext_{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let vault = VaultManager::unlocked_fixture(root.clone());
        let plain = rusqlite::Connection::open(root.join(ORIGINAL)).unwrap();
        plain.execute_batch("CREATE TABLE custom_data(id INTEGER PRIMARY KEY AUTOINCREMENT, value BLOB); CREATE VIEW custom_view AS SELECT * FROM custom_data; INSERT INTO custom_data(value) VALUES(X'0001FF'); CREATE TABLE private_note(value TEXT); INSERT INTO private_note VALUES('PRIVATE-RETIREMENT-SENTINEL');").unwrap();
        drop(plain);
        (root, vault)
    }

    #[test]
    fn explicit_retirement_keeps_verified_encrypted_original_and_live_vault() {
        let _serial = SERIAL.lock().unwrap();
        let (root, vault) = fixture();
        let key = vault.master_key().unwrap();
        let envelope = vault.envelope().unwrap();
        let id = retire(
            &root,
            &key,
            &envelope,
            "Synthetic-fixture-password",
            CONFIRMATION,
            || Ok(()),
        )
        .unwrap();
        assert!(!root.join(ORIGINAL).exists());
        assert!(vault.connection().unwrap().lock().is_ok());
        let archive = root.join("plaintext-retired").join(id);
        let saved_envelope = VaultEnvelope::load_from_dir(&archive).unwrap().unwrap();
        let saved_key =
            unwrap_master_key_with_password(&saved_envelope, "Synthetic-fixture-password").unwrap();
        let copy =
            open_encrypted_database(&archive.join("archive.db"), saved_key.as_bytes(), false)
                .unwrap();
        assert_eq!(
            copy.query_row("SELECT value FROM private_note", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "PRIVATE-RETIREMENT-SENTINEL"
        );
        assert_eq!(
            copy.query_row("SELECT value FROM custom_view", [], |r| r
                .get::<_, Vec<u8>>(0))
                .unwrap(),
            vec![0, 1, 255]
        );
        let bytes = fs::read(archive.join("archive.db")).unwrap();
        assert!(!bytes
            .windows(b"PRIVATE-RETIREMENT-SENTINEL".len())
            .any(|window| window == b"PRIVATE-RETIREMENT-SENTINEL"));
        assert!(archive.join("receipt.json").exists());
        assert!(!root.join("temp").exists());
        drop(copy);
        vault.lock().unwrap();
        drop(vault);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn large_multrow_plaintext_retirement_streams_without_memory_snapshot_limit() {
        let _serial = SERIAL.lock().unwrap();
        let (root, vault) = fixture();
        let plain_path = root.join(ORIGINAL);
        let mut plain = rusqlite::Connection::open(&plain_path).unwrap();
        plain
            .execute_batch(
                "CREATE TABLE large_history(id INTEGER PRIMARY KEY, url TEXT NOT NULL, payload BLOB NOT NULL);",
            )
            .unwrap();
        let tx = plain.transaction().unwrap();
        let payload = vec![b'P'; 8 * 1024];
        {
            let mut insert = tx
                .prepare("INSERT INTO large_history(id,url,payload) VALUES(?1,?2,?3)")
                .unwrap();
            for id in 0..36_000i64 {
                insert
                    .execute(rusqlite::params![
                        id,
                        format!("https://large-retirement.test/{id}"),
                        &payload,
                    ])
                    .unwrap();
            }
        }
        tx.commit().unwrap();
        drop(plain);
        assert!(fs::metadata(&plain_path).unwrap().len() > 256 * 1024 * 1024);

        let key = vault.master_key().unwrap();
        let envelope = vault.envelope().unwrap();
        let id = retire(
            &root,
            &key,
            &envelope,
            "Synthetic-fixture-password",
            CONFIRMATION,
            || Ok(()),
        )
        .unwrap();
        assert!(!plain_path.exists());
        let archive = root.join("plaintext-retired").join(id);
        let saved = VaultEnvelope::load_from_dir(&archive).unwrap().unwrap();
        let saved_key =
            unwrap_master_key_with_password(&saved, "Synthetic-fixture-password").unwrap();
        let encrypted =
            open_encrypted_database(&archive.join("archive.db"), saved_key.as_bytes(), false)
                .unwrap();
        let count: i64 = encrypted
            .query_row("SELECT count(*) FROM large_history", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 36_000);
        let payload_len: i64 = encrypted
            .query_row(
                "SELECT length(payload) FROM large_history WHERE id=35999",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(payload_len, 8 * 1024);
        assert!(check_encrypted_integrity(&encrypted).unwrap());
        drop(encrypted);
        vault.lock().unwrap();
        drop(vault);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refused_or_cancelled_retirement_preserves_plaintext_original() {
        let _serial = SERIAL.lock().unwrap();
        let (root, vault) = fixture();
        let key = vault.master_key().unwrap();
        let envelope = vault.envelope().unwrap();
        let before = fs::read(root.join(ORIGINAL)).unwrap();
        assert!(retire(
            &root,
            &key,
            &envelope,
            "wrong password",
            CONFIRMATION,
            || Ok(())
        )
        .is_err());
        assert!(retire(
            &root,
            &key,
            &envelope,
            "Synthetic-fixture-password",
            "",
            || Ok(())
        )
        .is_err());
        fs::write(root.join("archive.db-wal.plaintext.bak"), b"preserve").unwrap();
        assert!(inspect(&root).unwrap().blocked);
        assert!(retire(
            &root,
            &key,
            &envelope,
            "Synthetic-fixture-password",
            CONFIRMATION,
            || Ok(())
        )
        .is_err());
        fs::remove_file(root.join("archive.db-wal.plaintext.bak")).unwrap();
        let calls = std::cell::Cell::new(0);
        assert!(retire(
            &root,
            &key,
            &envelope,
            "Synthetic-fixture-password",
            CONFIRMATION,
            || {
                calls.set(calls.get() + 1);
                if calls.get() > 1 {
                    Err(refused("cancelled"))
                } else {
                    Ok(())
                }
            }
        )
        .is_err());
        assert_eq!(calls.get(), 2);
        assert_eq!(fs::read(root.join(ORIGINAL)).unwrap(), before);
        vault.lock().unwrap();
        drop(vault);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn changed_original_is_not_removed_after_archive_validation() {
        let _serial = SERIAL.lock().unwrap();
        let (root, vault) = fixture();
        let key = vault.master_key().unwrap();
        let envelope = vault.envelope().unwrap();
        let calls = std::cell::Cell::new(0);
        assert!(retire(
            &root,
            &key,
            &envelope,
            "Synthetic-fixture-password",
            CONFIRMATION,
            || {
                calls.set(calls.get() + 1);
                if calls.get() == 2 {
                    let conn = rusqlite::Connection::open(root.join(ORIGINAL))?;
                    conn.execute(
                        "UPDATE private_note SET value='changed after validation'",
                        [],
                    )?;
                }
                Ok(())
            }
        )
        .is_err());
        let original = rusqlite::Connection::open(root.join(ORIGINAL)).unwrap();
        assert_eq!(
            original
                .query_row("SELECT value FROM private_note", [], |r| r
                    .get::<_, String>(0))
                .unwrap(),
            "changed after validation"
        );
        drop(original);
        vault.lock().unwrap();
        drop(vault);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn lock_after_final_check_does_not_reclassify_committed_removal_as_failure() {
        use std::sync::Arc;
        use std::time::{Duration, Instant};
        let _serial = SERIAL.lock().unwrap();
        let (root, vault) = fixture();
        let vault = Arc::new(vault);
        let key = vault.master_key().unwrap();
        let envelope = vault.envelope().unwrap();
        let operation = vault.begin_operation().unwrap();
        let calls = std::cell::Cell::new(0);
        let locker = std::cell::RefCell::new(None);
        let result = retire(
            &root,
            &key,
            &envelope,
            "Synthetic-fixture-password",
            CONFIRMATION,
            || {
                operation.check()?;
                calls.set(calls.get() + 1);
                if calls.get() == 2 {
                    // Simulate revocation immediately AFTER the final check passed.
                    let closing = vault.clone();
                    *locker.borrow_mut() =
                        Some(std::thread::spawn(move || closing.lock().unwrap()));
                    let deadline = Instant::now() + Duration::from_secs(5);
                    while vault.status() != crate::security::vault::VaultStatus::Locking {
                        assert!(Instant::now() < deadline);
                        std::thread::yield_now();
                    }
                }
                Ok(())
            },
        );
        assert!(
            result.is_ok(),
            "the committed outcome must survive later revocation"
        );
        assert!(!root.join(ORIGINAL).exists());
        drop(operation);
        locker.into_inner().unwrap().join().unwrap();
        assert_eq!(vault.status(), crate::security::vault::VaultStatus::Locked);
        drop(vault);
        fs::remove_dir_all(root).unwrap();
    }
}
