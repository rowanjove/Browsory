//! Restart-recoverable installation of an already validated encrypted pair.
//! All file names are application-owned; journal strings never authorize paths.
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use tracing::warn;

use super::atomic_file::write_atomic;
use super::backup::compute_file_sha256;
use super::envelope::{MasterKey, VaultEnvelope};
use crate::database::encrypt_migration::is_plain_sqlite_database;
use crate::database::sqlcipher::{check_encrypted_integrity, open_encrypted_database};

pub const WORKSPACE: &str = "vault.restore";
const JOURNAL: &str = "restore.state.json";
const DB: &str = "archive.db";
const ENV: &str = "vault.envelope.json";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Stage {
    Prepared,
    OldDbMoved,
    OldEnvelopeMoved,
    NewDbInstalled,
    NewEnvelopeInstalled,
    Committed,
    RolledBack,
}

#[derive(Clone, Serialize, Deserialize)]
struct Journal {
    version: u32,
    id: uuid::Uuid,
    stage: Stage,
    old_db: String,
    old_envelope: String,
    new_db: String,
    new_envelope: String,
}

fn hash(path: &Path) -> Result<String, String> {
    compute_file_sha256(path).map_err(|e| e.to_string())
}
fn sync_file(path: &Path) -> Result<(), String> {
    fs::OpenOptions::new()
        .write(true)
        .open(path)
        .and_then(|file| file.sync_all())
        .map_err(|e| e.to_string())
}
fn matches(path: &Path, expected: &str) -> Result<bool, String> {
    if !path.is_file() {
        return Ok(false);
    }
    Ok(hash(path)? == expected)
}
fn save(directory: &Path, journal: &Journal) -> Result<(), String> {
    write_atomic(
        &directory.join(JOURNAL),
        &serde_json::to_vec(journal).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
fn backup_dir(directory: &Path, id: uuid::Uuid) -> PathBuf {
    directory.join("backups").join(format!("pre_restore_{id}"))
}

fn load(directory: &Path) -> Result<Option<Journal>, String> {
    let path = directory.join(JOURNAL);
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let mut bytes = Vec::new();
    file.take(16 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 16 * 1024 {
        return Err("恢复状态文件过大，保留现场".into());
    }
    let journal: Journal =
        serde_json::from_slice(&bytes).map_err(|e| format!("恢复状态文件损坏，保留现场: {e}"))?;
    if journal.version != 1 {
        return Err("不支持的恢复状态版本，保留现场".into());
    }
    for value in [
        &journal.old_db,
        &journal.old_envelope,
        &journal.new_db,
        &journal.new_envelope,
    ] {
        if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("恢复状态哈希格式错误，保留现场".into());
        }
    }
    Ok(Some(journal))
}

fn validate_pair(directory: &Path, key: &MasterKey) -> Result<(), String> {
    if is_plain_sqlite_database(&directory.join(DB)) {
        return Err("恢复文件必须是加密数据库".into());
    }
    VaultEnvelope::load_from_dir(directory)?.ok_or("恢复副本缺少信封")?;
    let conn = open_encrypted_database(&directory.join(DB), key.as_bytes(), false)
        .map_err(|e| e.to_string())?;
    if !check_encrypted_integrity(&conn).map_err(|e| e.to_string())? {
        return Err("恢复配对副本完整性检查失败".into());
    }
    let busy: i64 = conn
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if busy != 0 {
        return Err("恢复副本 WAL 无法合并".into());
    }
    drop(conn);
    sync_file(&directory.join(DB))?;
    Ok(())
}

fn terminal_files_exist(directory: &Path) -> Result<(), String> {
    if !directory.join(DB).is_file()
        || is_plain_sqlite_database(&directory.join(DB))
        || fs::metadata(directory.join(DB))
            .map_err(|e| e.to_string())?
            .len()
            < 16
    {
        return Err("已提交恢复的数据库缺失或格式不一致，保留现场".into());
    }
    VaultEnvelope::load_from_dir(directory)?.ok_or("已提交恢复的信封缺失")?;
    Ok(())
}

fn finish(directory: &Path, journal: &Journal) {
    let workspace = directory.join(WORKSPACE);
    if workspace.exists() {
        // Preserve old/new material rather than deleting the sole possible copy.
        let retained = directory.join("restore-recovery").join(format!(
            "{}_{}",
            journal.id,
            uuid::Uuid::new_v4()
        ));
        if let Err(error) = fs::create_dir_all(retained.parent().unwrap())
            .and_then(|_| fs::rename(&workspace, &retained))
        {
            warn!("已提交恢复，但暂存材料尚未归档: {}", error);
            return;
        }
    }
    if let Err(error) = fs::remove_file(directory.join(JOURNAL)) {
        warn!("已提交恢复状态记录尚未清理: {}", error);
    }
}

/// Called under directory ownership before any connection or orphan cleanup.
/// Nonterminal operations always roll back to their verified pre-restore pair.
pub fn recover(directory: &Path) -> Result<bool, String> {
    let mut journal = match load(directory)? {
        Some(journal) => journal,
        None => {
            let workspace = directory.join(WORKSPACE);
            if workspace.exists() {
                // No live-file mutation is allowed before a durable journal.
                // Preserve an interrupted extraction without treating it as a vault.
                let retained = directory
                    .join("restore-recovery")
                    .join(format!("uncommitted_{}", uuid::Uuid::new_v4()));
                fs::create_dir_all(retained.parent().unwrap()).map_err(|e| e.to_string())?;
                fs::rename(workspace, retained).map_err(|e| e.to_string())?;
                return Ok(true);
            }
            return Ok(false);
        }
    };
    if matches!(journal.stage, Stage::Committed | Stage::RolledBack) {
        terminal_files_exist(directory)?;
        finish(directory, &journal);
        return Ok(true);
    }
    let previous = backup_dir(directory, journal.id);
    let mut sources = Vec::new();
    for (name, expected) in [(DB, &journal.old_db), (ENV, &journal.old_envelope)] {
        let mut found = None;
        for candidate in [
            previous.join(name),
            directory.join(WORKSPACE).join("previous").join(name),
            directory.join(name),
        ] {
            if matches(&candidate, expected)? {
                found = Some(candidate);
                break;
            }
        }
        sources.push(found.ok_or("旧配对文件在所有受控位置均缺失或哈希不符，保留现场")?);
    }
    // A rollback is repeatable after another crash. An already-restored file is
    // not touched; each other live file is retained under a fresh unique name.
    let rollback = directory.join(WORKSPACE).join("rollback");
    fs::create_dir_all(&rollback).map_err(|e| e.to_string())?;
    for ((name, expected), source) in [(DB, &journal.old_db), (ENV, &journal.old_envelope)]
        .into_iter()
        .zip(sources)
    {
        let target = directory.join(name);
        if matches(&target, expected)? {
            continue;
        }
        let copy = rollback.join(name);
        fs::copy(source, &copy).map_err(|e| e.to_string())?;
        sync_file(&copy)?;
        if !matches(&copy, expected)? {
            return Err("回滚暂存副本校验失败".into());
        }
        if target.exists() {
            let retained = directory
                .join("restore-recovery")
                .join(uuid::Uuid::new_v4().to_string());
            fs::create_dir_all(&retained).map_err(|e| e.to_string())?;
            fs::rename(&target, retained.join(name)).map_err(|e| e.to_string())?;
        }
        fs::rename(copy, target).map_err(|e| e.to_string())?;
    }
    // Sidecars must not be attached to the restored database. Preserve them.
    for name in ["archive.db-wal", "archive.db-shm"] {
        let path = directory.join(name);
        if path.exists() {
            let retained = directory
                .join("restore-recovery")
                .join(uuid::Uuid::new_v4().to_string());
            fs::create_dir_all(&retained).map_err(|e| e.to_string())?;
            fs::rename(path, retained.join(name)).map_err(|e| e.to_string())?;
        }
    }
    if !matches(&directory.join(DB), &journal.old_db)?
        || !matches(&directory.join(ENV), &journal.old_envelope)?
    {
        return Err("恢复回滚未能得到完整旧配对，保留现场".into());
    }
    journal.stage = Stage::RolledBack;
    save(directory, &journal)?;
    finish(directory, &journal);
    Ok(true)
}

pub fn install(
    directory: &Path,
    old_key: &MasterKey,
    old_envelope: &VaultEnvelope,
    new_key: &MasterKey,
) -> Result<(), String> {
    match install_observed(directory, old_key, old_envelope, new_key, |_| Ok(())) {
        Ok(()) => Ok(()),
        Err(error) => {
            let committed = load(directory)?
                .map(|journal| journal.stage == Stage::Committed)
                .unwrap_or(false);
            recover(directory)?;
            if committed {
                Ok(())
            } else {
                Err(format!("恢复未提交，已保留或回滚原配对: {error}"))
            }
        }
    }
}

fn install_observed<F>(
    directory: &Path,
    old_key: &MasterKey,
    old_envelope: &VaultEnvelope,
    new_key: &MasterKey,
    mut observe: F,
) -> Result<(), String>
where
    F: FnMut(&str) -> Result<(), String>,
{
    if directory.join(JOURNAL).exists() {
        return Err("存在尚未处理的恢复状态，拒绝覆盖".into());
    }
    let workspace = directory.join(WORKSPACE);
    let incoming = workspace.join("incoming");
    validate_pair(&incoming, new_key)?;
    if VaultEnvelope::load_from_dir(directory)?.as_ref() != Some(old_envelope) {
        return Err("当前保险库凭据已变化，恢复已取消，请重新解锁后重试".into());
    }
    // Callback must already have drained and closed every active connection.
    if fs::metadata(directory.join("archive.db-wal"))
        .map(|meta| meta.len() > 0)
        .unwrap_or(false)
    {
        return Err("旧数据库 WAL 尚未合并，禁止开始恢复覆盖".into());
    }
    let id = uuid::Uuid::new_v4();
    let previous = backup_dir(directory, id);
    let partial = directory
        .join("backups")
        .join(format!(".pre_restore_{id}.partial"));
    fs::create_dir_all(&partial).map_err(|e| e.to_string())?;
    for name in [DB, ENV] {
        fs::copy(directory.join(name), partial.join(name)).map_err(|e| e.to_string())?;
        sync_file(&partial.join(name))?;
    }
    validate_pair(&partial, old_key)?;
    fs::rename(&partial, &previous).map_err(|e| e.to_string())?;
    let mut journal = Journal {
        version: 1,
        id,
        stage: Stage::Prepared,
        old_db: hash(&previous.join(DB))?,
        old_envelope: hash(&previous.join(ENV))?,
        new_db: hash(&incoming.join(DB))?,
        new_envelope: hash(&incoming.join(ENV))?,
    };
    if !matches(&directory.join(DB), &journal.old_db)?
        || !matches(&directory.join(ENV), &journal.old_envelope)?
    {
        return Err("恢复前当前文件发生变化，已中止切换".into());
    }
    save(directory, &journal)?;
    observe("prepared")?;
    let retired = workspace.join("previous");
    fs::create_dir_all(&retired).map_err(|e| e.to_string())?;
    for (name, stage, from, to, point) in [
        (
            DB,
            Stage::OldDbMoved,
            directory.to_path_buf(),
            retired.clone(),
            "old_db_moved",
        ),
        (
            ENV,
            Stage::OldEnvelopeMoved,
            directory.to_path_buf(),
            retired.clone(),
            "old_envelope_moved",
        ),
        (
            DB,
            Stage::NewDbInstalled,
            incoming.clone(),
            directory.to_path_buf(),
            "new_db_installed",
        ),
        (
            ENV,
            Stage::NewEnvelopeInstalled,
            incoming.clone(),
            directory.to_path_buf(),
            "new_envelope_installed",
        ),
    ] {
        fs::rename(from.join(name), to.join(name)).map_err(|e| e.to_string())?;
        observe(point)?; // deliberately before journal update for crash tests
        journal.stage = stage;
        save(directory, &journal)?;
    }
    if !matches(&directory.join(DB), &journal.new_db)?
        || !matches(&directory.join(ENV), &journal.new_envelope)?
    {
        return Err("安装后的数据库与信封校验失败".into());
    }
    journal.stage = Stage::Committed;
    save(directory, &journal)?;
    observe("committed")?;
    finish(directory, &journal);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::envelope::{create_vault_envelope, KdfParameters, MIN_MEM_COST_KB};

    fn envelope(password: &str) -> (VaultEnvelope, MasterKey) {
        let parameters = KdfParameters {
            mem_cost_kb: MIN_MEM_COST_KB,
            time_cost: 1,
            parallelism: 1,
        };
        let (envelope, _, key) = create_vault_envelope(password, Some(parameters), 0).unwrap();
        (envelope, key)
    }
    fn pair(path: &Path, envelope: &VaultEnvelope, key: &MasterKey, value: &str) {
        fs::create_dir_all(path).unwrap();
        let conn = open_encrypted_database(&path.join(DB), key.as_bytes(), true).unwrap();
        conn.execute_batch("CREATE TABLE sentinel(value TEXT);")
            .unwrap();
        conn.execute("INSERT INTO sentinel VALUES(?1)", [value])
            .unwrap();
        drop(conn);
        envelope.save_atomic(path).unwrap();
    }
    fn value(path: &Path, key: &MasterKey) -> String {
        let conn = open_encrypted_database(&path.join(DB), key.as_bytes(), false).unwrap();
        assert!(check_encrypted_integrity(&conn).unwrap());
        conn.query_row("SELECT value FROM sentinel", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn restart_recovers_every_cutover_window_without_pair_mismatch() {
        let (old_envelope, old_key) = envelope("Old-restore-passphrase");
        let (new_envelope, new_key) = envelope("New-restore-passphrase");
        for point in [
            "prepared",
            "old_db_moved",
            "old_envelope_moved",
            "new_db_installed",
            "new_envelope_installed",
            "committed",
        ] {
            let root = std::env::temp_dir()
                .join(format!("browsory_restore_window_{}", uuid::Uuid::new_v4()));
            pair(&root, &old_envelope, &old_key, "old");
            pair(
                &root.join(WORKSPACE).join("incoming"),
                &new_envelope,
                &new_key,
                "new",
            );
            let result = install_observed(&root, &old_key, &old_envelope, &new_key, |current| {
                if current == point {
                    Err("simulated process exit".into())
                } else {
                    Ok(())
                }
            });
            assert!(result.is_err());
            let manager = crate::security::vault::VaultManager::new(root.clone());
            assert_eq!(
                manager.status(),
                crate::security::vault::VaultStatus::Locked,
                "{point}"
            );
            let expected_key = if point == "committed" {
                &new_key
            } else {
                &old_key
            };
            assert_eq!(
                value(&root, expected_key),
                if point == "committed" { "new" } else { "old" },
                "{point}"
            );
            assert_eq!(
                VaultEnvelope::load_from_dir(&root).unwrap().unwrap(),
                if point == "committed" {
                    new_envelope.clone()
                } else {
                    old_envelope.clone()
                }
            );
            let old_backup = fs::read_dir(root.join("backups"))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
            assert_eq!(value(&old_backup, &old_key), "old");
            assert!(!root.join(JOURNAL).exists());
            assert!(!recover(&root).unwrap());
            drop(manager);
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn corrupted_journal_preserves_all_files_and_fails_closed() {
        let root = std::env::temp_dir().join(format!(
            "browsory_restore_bad_journal_{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(root.join(WORKSPACE)).unwrap();
        fs::write(root.join(JOURNAL), b"{broken").unwrap();
        fs::write(root.join(DB), b"must preserve current bytes").unwrap();
        fs::write(
            root.join(WORKSPACE).join("evidence"),
            b"must preserve staging",
        )
        .unwrap();
        assert!(recover(&root).is_err());
        assert_eq!(
            fs::read(root.join(DB)).unwrap(),
            b"must preserve current bytes"
        );
        assert!(root.join(WORKSPACE).join("evidence").exists());
        let manager = crate::security::vault::VaultManager::new(root.clone());
        assert!(matches!(
            manager.status(),
            crate::security::vault::VaultStatus::Error(_)
        ));
        drop(manager);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn committed_cleanup_does_not_rollback_subsequent_legitimate_writes() {
        let (old_envelope, old_key) = envelope("Old-restore-passphrase");
        let (new_envelope, new_key) = envelope("New-restore-passphrase");
        let root = std::env::temp_dir().join(format!(
            "browsory_restore_committed_{}",
            uuid::Uuid::new_v4()
        ));
        pair(&root, &old_envelope, &old_key, "old");
        pair(
            &root.join(WORKSPACE).join("incoming"),
            &new_envelope,
            &new_key,
            "new",
        );
        assert!(
            install_observed(&root, &old_key, &old_envelope, &new_key, |point| {
                if point == "committed" {
                    Err("cleanup interrupted".into())
                } else {
                    Ok(())
                }
            })
            .is_err()
        );
        {
            let conn = open_encrypted_database(&root.join(DB), new_key.as_bytes(), false).unwrap();
            conn.execute("UPDATE sentinel SET value='later write'", [])
                .unwrap();
        }
        recover(&root).unwrap();
        assert_eq!(value(&root, &new_key), "later write");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn verified_retired_original_recovers_if_extra_backup_copy_is_lost() {
        let (old_envelope, old_key) = envelope("Old-restore-passphrase");
        let (new_envelope, new_key) = envelope("New-restore-passphrase");
        let root =
            std::env::temp_dir().join(format!("browsory_restore_retired_{}", uuid::Uuid::new_v4()));
        pair(&root, &old_envelope, &old_key, "old");
        pair(
            &root.join(WORKSPACE).join("incoming"),
            &new_envelope,
            &new_key,
            "new",
        );
        assert!(
            install_observed(&root, &old_key, &old_envelope, &new_key, |point| {
                if point == "new_db_installed" {
                    Err("interrupted".into())
                } else {
                    Ok(())
                }
            })
            .is_err()
        );
        let journal = load(&root).unwrap().unwrap();
        fs::remove_file(backup_dir(&root, journal.id).join(DB)).unwrap();
        recover(&root).unwrap();
        assert_eq!(value(&root, &old_key), "old");
        fs::remove_dir_all(root).unwrap();
    }
}
