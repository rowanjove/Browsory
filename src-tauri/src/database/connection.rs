use crate::security::directory_lock::DataDirectoryLock;
use crate::security::session::{ConnectionHandle, VaultOperation};
use directories::ProjectDirs;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::info;

use crate::error::{AppError, AppResult};
use crate::security::vault::VaultManager;

#[derive(Clone)]
pub struct DbState {
    pub(crate) _directory_lock: Option<Arc<DataDirectoryLock>>,
    pub vault: Arc<VaultManager>,
    pub app_dir: PathBuf,
    pub temp_dir: PathBuf,
    pub logs_dir: PathBuf,
}

impl DbState {
    pub fn with_conn<F, R>(&self, f: F) -> AppResult<R>
    where
        F: FnOnce(&rusqlite::Connection) -> AppResult<R>,
    {
        self.vault.with_conn(f)
    }

    pub fn with_conn_mut<F, R>(&self, f: F) -> AppResult<R>
    where
        F: FnOnce(&mut rusqlite::Connection) -> AppResult<R>,
    {
        self.vault.with_conn_mut(f)
    }

    pub fn with_read_conn<F, R>(&self, f: F) -> AppResult<R>
    where
        F: FnOnce(&rusqlite::Connection) -> AppResult<R>,
    {
        self.vault.with_read_conn(f)
    }

    pub fn get_connection(&self) -> AppResult<ConnectionHandle> {
        self.vault.connection()
    }

    pub fn begin_operation(&self) -> AppResult<VaultOperation> {
        self.vault.begin_operation()
    }
}

pub fn is_portable_install(exe_dir: &Path) -> bool {
    exe_dir.join("portable.txt").is_file() || exe_dir.join("portable").is_file()
}

pub fn resolve_data_directory(exe_dir: Option<&Path>) -> AppResult<PathBuf> {
    if let Some(dir) = exe_dir {
        if is_portable_install(dir) {
            return Ok(dir.join("data"));
        }
    }
    let proj_dirs = ProjectDirs::from("com", "browsory", "app")
        .ok_or_else(|| AppError::System("Failed to resolve project directories".into()))?;
    Ok(proj_dirs.data_local_dir().to_path_buf())
}

pub fn get_app_directories() -> AppResult<(PathBuf, PathBuf, PathBuf, PathBuf)> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()));
    let data_dir = resolve_data_directory(exe_dir.as_deref())?;
    let temp_dir = data_dir.join("temp");
    let logs_dir = data_dir.join("logs");
    let db_path = data_dir.join("archive.db");

    fs::create_dir_all(&data_dir)?;
    fs::create_dir_all(&temp_dir)?;
    fs::create_dir_all(&logs_dir)?;

    Ok((data_dir, temp_dir, logs_dir, db_path))
}

pub fn init_database() -> AppResult<DbState> {
    let (app_dir, temp_dir, logs_dir, _db_path) = get_app_directories()?;
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf));
    let legacy = legacy_import_source(exe_dir.as_deref());
    initialize_at(app_dir, temp_dir, logs_dir, legacy.as_deref())
}

fn legacy_import_source(exe_dir: Option<&Path>) -> Option<PathBuf> {
    // A new portable directory must remain independent of the host's installed
    // legacy archive. Explicit file import remains available after unlocking.
    if exe_dir.is_some_and(is_portable_install) {
        return None;
    }
    ProjectDirs::from("com", "browserhistory", "archive")
        .map(|dirs| dirs.data_local_dir().join("archive.db"))
}

fn initialize_at(
    app_dir: PathBuf,
    temp_dir: PathBuf,
    logs_dir: PathBuf,
    legacy: Option<&Path>,
) -> AppResult<DbState> {
    let directory_lock = Arc::new(DataDirectoryLock::acquire(&app_dir)?);
    crate::history::record_spool::cleanup_orphans(&app_dir, &directory_lock);
    import_legacy_if_absent(&app_dir, legacy)?;
    info!("Initializing archive vault manager at: {:?}", app_dir);

    // Sanitize any leftover orphan snapshots from prior sessions
    let (cleaned_count, freed_bytes) =
        crate::history::snapshot::cleanup_orphan_snapshots(&app_dir, &temp_dir);
    if cleaned_count > 0 {
        info!(
            "启动自检清理残留孤儿快照: {} 项 ({} 字节)",
            cleaned_count, freed_bytes
        );
    }

    let vault = Arc::new(VaultManager::new(app_dir.clone()));
    #[cfg(target_os = "windows")]
    if let Err(err) = vault.ensure_passive_vault_unlocked() {
        tracing::warn!("Passive vault auto-unlock deferred: {}", err);
    }
    info!(
        "Step 1: VaultManager initialized with status: {:?}",
        vault.status()
    );

    // Passive DPAPI vaults auto-initialize/unlock unless a 4-digit UI PIN is enabled.
    // Legacy passphrase-only vaults remain Locked until unlocked once to enroll DPAPI.

    Ok(DbState {
        _directory_lock: Some(directory_lock),
        vault,
        app_dir,
        temp_dir,
        logs_dir,
    })
}

fn import_legacy_if_absent(app_dir: &Path, legacy: Option<&Path>) -> AppResult<()> {
    if app_dir.join("archive.db").exists() {
        return Ok(());
    }
    for artifact in [
        "vault.envelope.json",
        "migration.state.json",
        "restore.state.json",
        "archive.staging.db",
        "vault.restore",
    ] {
        if app_dir.join(artifact).exists() {
            return Ok(());
        }
    }
    let staging = app_dir.join("archive.legacy.staging.db");
    if staging.exists() {
        return Err(AppError::Security(
            "存在未完成的旧目录导入，已保留现场，拒绝创建空库".into(),
        ));
    }
    let Some(source) = legacy.filter(|path| path.is_file()) else {
        return Ok(());
    };
    if !crate::database::encrypt_migration::is_plain_sqlite_database(source) {
        return Err(AppError::Security(
            "旧目录数据库不是明文 SQLite，禁止无信封自动导入".into(),
        ));
    }
    let conn =
        rusqlite::Connection::open_with_flags(source, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    super::sqlcipher::configure_private_connection(&conn)?;
    // VACUUM INTO reads a transactionally consistent snapshot, including WAL.
    conn.execute("VACUUM main INTO ?1", [staging.to_string_lossy().as_ref()])?;
    if !crate::database::encrypt_migration::verify_sqlite_integrity(&staging)? {
        return Err(AppError::Security(
            "旧目录导入副本校验失败，原件未修改".into(),
        ));
    }
    fs::OpenOptions::new()
        .write(true)
        .open(&staging)?
        .sync_all()?;
    fs::rename(staging, app_dir.join("archive.db"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ownership_precedes_orphan_cleanup_and_release_allows_restart() {
        let root =
            std::env::temp_dir().join(format!("browsory_owned_startup_{}", uuid::Uuid::new_v4()));
        let temporary = root.join("temp");
        fs::create_dir_all(&temporary).unwrap();
        let first =
            initialize_at(root.clone(), temporary.clone(), root.join("logs"), None).unwrap();
        let sentinel = temporary.join("owned-by-running-instance");
        fs::write(&sentinel, b"synthetic").unwrap();
        assert!(initialize_at(root.clone(), temporary.clone(), root.join("logs"), None).is_err());
        assert!(
            sentinel.exists(),
            "second instance must not clean the first instance's work"
        );
        drop(first);
        let restarted = initialize_at(root.clone(), temporary, root.join("logs"), None).unwrap();
        assert!(sentinel.exists(), "未知顶层临时项必须保留");
        drop(restarted);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_import_includes_live_wal_without_modifying_source() {
        let root =
            std::env::temp_dir().join(format!("browsory_legacy_wal_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("old.db");
        let conn = rusqlite::Connection::open(&source).unwrap();
        conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get::<_, String>(0))
            .unwrap();
        conn.query_row("PRAGMA wal_autocheckpoint=0", [], |r| r.get::<_, i64>(0))
            .unwrap();
        conn.execute_batch("CREATE TABLE t(value TEXT); INSERT INTO t VALUES('latest WAL data');")
            .unwrap();
        let old_bytes = fs::read(&source).unwrap();
        let old_wal = fs::read(source.with_extension("db-wal")).unwrap();
        let target = root.join("new");
        fs::create_dir_all(&target).unwrap();
        import_legacy_if_absent(&target, Some(&source)).unwrap();
        let copy = rusqlite::Connection::open(target.join("archive.db")).unwrap();
        assert_eq!(
            copy.query_row("SELECT value FROM t", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "latest WAL data"
        );
        assert_eq!(fs::read(&source).unwrap(), old_bytes);
        assert_eq!(fs::read(source.with_extension("db-wal")).unwrap(), old_wal);
        drop(copy);
        drop(conn);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_current_database_with_vault_artifacts_does_not_import_legacy() {
        let root = std::env::temp_dir().join(format!(
            "browsory_no_legacy_override_{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("old.db");
        {
            let conn = rusqlite::Connection::open(&source).unwrap();
            conn.execute_batch("CREATE TABLE t(id);").unwrap();
        }
        fs::write(root.join("vault.envelope.json"), b"preserve").unwrap();
        import_legacy_if_absent(&root, Some(&source)).unwrap();
        assert!(!root.join("archive.db").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn portable_marker_uses_exe_side_data_dir() {
        let root = std::env::temp_dir().join(format!(
            "browsory_portable_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("portable.txt"), b"portable").unwrap();
        let data = resolve_data_directory(Some(&root)).unwrap();
        assert_eq!(data, root.join("data"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn installed_mode_does_not_use_exe_dir_without_marker() {
        let root = std::env::temp_dir().join(format!(
            "browsory_installed_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let data = resolve_data_directory(Some(&root)).unwrap();
        assert_ne!(data, root.join("data"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn portable_startup_never_selects_the_hosts_legacy_archive() {
        let root = std::env::temp_dir().join(format!(
            "browsory_portable_no_host_import_{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("portable.txt"), b"portable").unwrap();
        assert!(legacy_import_source(Some(&root)).is_none());
        fs::remove_dir_all(root).unwrap();
    }
}
