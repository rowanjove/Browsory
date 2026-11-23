//! Large-source preparation with a private encrypted record spool, never a
//! plaintext database copy. The random key is deliberately not persisted.
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rusqlite::{params, Connection, OpenFlags};
use sha2::{Digest, Sha256};

use super::parser::{limited_text, RawVisitRecord};
use crate::database::space::{protected_snapshot_requirement, require_available_space};
use crate::database::sqlcipher::{
    check_encrypted_integrity, configure_private_connection, open_encrypted_database,
};
use crate::error::{AppError, AppResult};
use crate::security::envelope::MasterKey;
use crate::security::fs_boundary::{plain_metadata, resolve_scoped_path};

pub(crate) type CancelProbe = Arc<dyn Fn() -> bool + Send + Sync>;
pub(crate) const MEMORY_SOURCE_LIMIT: u64 = 256 * 1024 * 1024;
const MAX_SOURCE_BYTES: u64 = 64 * 1024 * 1024 * 1024;
const MAX_SPOOL_BYTES: u64 = 16 * 1024 * 1024 * 1024;
const WORK: &str = "snapshot-work";
const DB: &str = "records.db";
const OWNED_FILES: [&str; 4] = [DB, "records.db-wal", "records.db-shm", "records.db-journal"];

fn check(cancelled: &CancelProbe, deadline: Instant) -> AppResult<()> {
    if cancelled() {
        return Err(AppError::VaultLocked(
            "保险库会话已结束，已取消大库准备".into(),
        ));
    }
    if Instant::now() >= deadline {
        return Err(AppError::Browser(
            "大库准备超时，已中止且不会回退到明文副本".into(),
        ));
    }
    Ok(())
}

enum SpoolSource {
    Direct(Connection),
    Snapshot(super::snapshot::SnapshotGuard),
    #[cfg(windows)]
    NoLock {
        conn: Connection,
        path: PathBuf,
        pre_stamp: super::snapshot::FileStamp,
        pre_journal: super::snapshot::FileStamp,
        pre_wal: super::snapshot::FileStamp,
    },
}

impl std::ops::Deref for SpoolSource {
    type Target = Connection;
    fn deref(&self) -> &Self::Target {
        match self {
            SpoolSource::Direct(conn) => conn,
            SpoolSource::Snapshot(guard) => guard.connection(),
            #[cfg(windows)]
            SpoolSource::NoLock { conn, .. } => conn,
        }
    }
}

impl SpoolSource {
    fn finish(self) -> AppResult<()> {
        match self {
            SpoolSource::Direct(conn) => {
                conn.execute_batch("ROLLBACK")?;
                drop(conn);
                Ok(())
            }
            SpoolSource::Snapshot(guard) => {
                drop(guard);
                Ok(())
            }
            #[cfg(windows)]
            SpoolSource::NoLock {
                conn,
                path,
                pre_stamp,
                pre_journal,
                pre_wal,
            } => {
                drop(conn);
                let journal_path = super::snapshot::sidecar_path(&path, "-journal");
                let post_stamp = super::snapshot::read_file_stamp(&path)?;
                let post_journal = super::snapshot::read_file_stamp(&journal_path)?;
                let post_wal = super::snapshot::read_file_stamp(&super::snapshot::sidecar_path(
                    &path, "-wal",
                ))?;
                if post_stamp != pre_stamp
                    || post_journal != pre_journal
                    || post_wal != pre_wal
                    || super::snapshot::has_active_rollback_journal(&journal_path)?
                {
                    return Err(AppError::Browser(
                        "读取大体积浏览器历史库期间源文件发生变更，请稍后重试".into(),
                    ));
                }
                Ok(())
            }
        }
    }
}

fn configure_source_reader(
    source: &mut Connection,
    cancelled: CancelProbe,
    deadline: Instant,
) -> AppResult<()> {
    configure_private_connection(source)?;
    source.busy_timeout(Duration::from_millis(100))?;
    source.pragma_update(None, "cache_size", -8192)?;
    source.pragma_update(None, "automatic_index", false)?;
    let _: u64 = source.query_row("PRAGMA mmap_size=0", [], |r| r.get(0))?;
    source.progress_handler(
        10_000,
        Some(move || cancelled() || Instant::now() >= deadline),
    )?;
    source.execute_batch("BEGIN DEFERRED")?;
    Ok(())
}

fn private_reader(path: &Path, cancelled: CancelProbe, deadline: Instant) -> AppResult<Connection> {
    check(&cancelled, deadline)?;
    let mut source = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    configure_source_reader(&mut source, cancelled, deadline)?;
    Ok(source)
}

fn open_spool_source(
    path: &Path,
    temp: &Path,
    cancelled: CancelProbe,
    deadline: Instant,
) -> AppResult<SpoolSource> {
    match private_reader(path, cancelled.clone(), deadline) {
        Ok(conn) => Ok(SpoolSource::Direct(conn)),
        Err(ref err) if super::snapshot::is_app_error_locked(err) => {
            check(&cancelled, deadline)?;
            let meta_len = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            if meta_len > 0 && meta_len <= 256 * 1024 * 1024 {
                if let Ok(guard) =
                    super::snapshot::create_history_snapshot_checked(path, temp, cancelled.as_ref())
                {
                    return Ok(SpoolSource::Snapshot(guard));
                }
            }
            check(&cancelled, deadline)?;
            #[cfg(windows)]
            {
                let journal_path = super::snapshot::sidecar_path(path, "-journal");
                let wal_path = super::snapshot::sidecar_path(path, "-wal");
                if super::snapshot::has_active_rollback_journal(&journal_path)? {
                    return Err(AppError::Browser(
                        "浏览器历史库正在写入事务中，请稍后重试".into(),
                    ));
                }
                if super::snapshot::has_committed_wal_frames(&wal_path)? {
                    return Err(AppError::Browser(
                        "大体积 WAL 历史库正在占用中，请稍后重试".into(),
                    ));
                }
                let pre_stamp = super::snapshot::read_file_stamp(path)?;
                let pre_journal = super::snapshot::read_file_stamp(&journal_path)?;
                let pre_wal = super::snapshot::read_file_stamp(&wal_path)?;
                let mut source = Connection::open_with_flags_and_vfs(
                    path,
                    OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
                    "win32-none",
                )?;
                configure_source_reader(&mut source, cancelled, deadline)?;
                Ok(SpoolSource::NoLock {
                    conn: source,
                    path: path.to_path_buf(),
                    pre_stamp,
                    pre_journal,
                    pre_wal,
                })
            }
            #[cfg(not(windows))]
            {
                Err(AppError::Browser("数据库处于锁定状态".into()))
            }
        }
        Err(err) => Err(err),
    }
}

fn logical_size(source: &Connection) -> AppResult<u64> {
    let pages: u64 = source.pragma_query_value(None, "page_count", |r| r.get(0))?;
    let page_size: u64 = source.pragma_query_value(None, "page_size", |r| r.get(0))?;
    let size = pages
        .checked_mul(page_size)
        .ok_or_else(|| AppError::Security("源库大小溢出".into()))?;
    if size == 0 || size > MAX_SOURCE_BYTES {
        return Err(AppError::Security("源库为空或超过 64 GiB 限制".into()));
    }
    Ok(size)
}

pub(crate) fn needs_disk_spool(
    path: &Path,
    threshold: u64,
    cancelled: CancelProbe,
) -> AppResult<bool> {
    match private_reader(path, cancelled, Instant::now() + Duration::from_secs(10)) {
        Ok(source) => Ok(logical_size(&source)? > threshold),
        Err(ref err) if super::snapshot::is_app_error_locked(err) => {
            let db_len = fs::metadata(path)?.len();
            let wal_len = fs::metadata(super::snapshot::sidecar_path(path, "-wal"))
                .map(|m| m.len())
                .unwrap_or(0);
            let total = db_len.saturating_add(wal_len);
            if total == 0 || total > MAX_SOURCE_BYTES {
                return Err(AppError::Security("源库为空或超过 64 GiB 限制".into()));
            }
            Ok(total > threshold)
        }
        Err(err) => Err(err),
    }
}

fn remove_owned_workspace(root: &Path, relative: &Path) -> AppResult<()> {
    let Some(path) = resolve_scoped_path(root, relative)? else {
        return Ok(());
    };
    if !plain_metadata(&path)?.is_some_and(|meta| meta.is_dir()) {
        return Err(AppError::Security("加密暂存目录类型异常，保留现场".into()));
    }
    // Inspect all entries before any deletion, preserving unknown material.
    for entry in fs::read_dir(&path)? {
        let entry = entry?;
        if !entry
            .file_name()
            .to_str()
            .is_some_and(|name| OWNED_FILES.contains(&name))
            || !plain_metadata(&entry.path())?.is_some_and(|meta| meta.is_file())
        {
            return Err(AppError::Security(
                "加密暂存目录存在未知对象，保留现场".into(),
            ));
        }
    }
    for name in OWNED_FILES {
        if let Some(file) = resolve_scoped_path(root, &relative.join(name))? {
            fs::remove_file(file)?;
        }
    }
    fs::remove_dir(path)?;
    Ok(())
}

struct Workspace {
    root: PathBuf,
    relative: PathBuf,
}
impl Workspace {
    fn create(temp: &Path) -> AppResult<Self> {
        if temp.file_name() != Some(std::ffi::OsStr::new("temp")) {
            return Err(AppError::Security("大库暂存路径不是应用 temp 目录".into()));
        }
        let root = temp
            .parent()
            .ok_or_else(|| AppError::Security("应用目录缺失".into()))?
            .to_path_buf();
        if !plain_metadata(&root)?.is_some_and(|meta| meta.is_dir()) {
            return Err(AppError::Security("应用目录无效".into()));
        }
        // Validate the application's root and existing components before the
        // first mutation (including refusing a filesystem root or a link).
        if resolve_scoped_path(&root, Path::new(WORK))?.is_none() {
            fs::create_dir(root.join(WORK))?;
        }
        let base = resolve_scoped_path(&root, Path::new(WORK))?
            .ok_or_else(|| AppError::Security("加密暂存目录缺失".into()))?;
        if !plain_metadata(&base)?.is_some_and(|meta| meta.is_dir()) {
            return Err(AppError::Security("加密暂存根目录无效".into()));
        }
        let root = fs::canonicalize(root)?;
        let relative = Path::new(WORK).join(uuid::Uuid::new_v4().to_string());
        fs::create_dir(root.join(&relative))?;
        Ok(Self { root, relative })
    }
    fn enforce_quota(&self, limit: u64) -> AppResult<u64> {
        let path = resolve_scoped_path(&self.root, &self.relative)?
            .ok_or_else(|| AppError::Security("加密暂存目录消失".into()))?;
        let mut total = 0u64;
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            if !entry
                .file_name()
                .to_str()
                .is_some_and(|name| OWNED_FILES.contains(&name))
            {
                return Err(AppError::Security("加密暂存目录存在未知文件".into()));
            }
            let metadata = plain_metadata(&entry.path())?
                .ok_or_else(|| AppError::Security("加密暂存文件消失".into()))?;
            if !metadata.is_file() {
                return Err(AppError::Security("加密暂存包含非普通文件".into()));
            }
            total = total
                .checked_add(metadata.len())
                .ok_or_else(|| AppError::Security("加密暂存占用溢出".into()))?;
            if total > limit {
                return Err(AppError::Security("加密暂存工作区总占用超过限制".into()));
            }
        }
        Ok(total)
    }
    fn db(&self) -> PathBuf {
        self.root.join(&self.relative).join(DB)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        if remove_owned_workspace(&self.root, &self.relative).is_err() {
            tracing::warn!("加密记录暂存尚未清理；临时密钥不会持久保存");
        }
    }
}

pub(crate) struct RecordSpool {
    // Close SQLCipher before the key and workspace are released.
    connection: Connection,
    _key: MasterKey,
    _workspace: Workspace,
    count: u32,
    cursor_regressed: bool,
    #[cfg(test)]
    peak_bytes: u64,
    cancelled: CancelProbe,
}

fn record_hash(hasher: &mut Sha256, record: &RawVisitRecord) {
    for value in [
        record.source_visit_id,
        record.visit_time_unix_ms,
        record.from_visit,
        record.transition,
        record.visit_duration,
    ] {
        hasher.update(value.to_le_bytes());
    }
    for text in [&record.url, &record.title] {
        hasher.update((text.len() as u64).to_le_bytes());
        hasher.update(text.as_bytes());
    }
}

impl RecordSpool {
    pub(crate) fn prepare(
        source_path: &Path,
        temp: &Path,
        is_firefox: bool,
        since: i64,
        cancelled: CancelProbe,
    ) -> AppResult<Self> {
        Self::prepare_with_schema(
            source_path,
            temp,
            Some(is_firefox),
            since,
            0,
            cancelled,
            MAX_SPOOL_BYTES,
            Duration::from_secs(300),
        )
    }

    pub(crate) fn prepare_auto_with_cursor_check(
        source_path: &Path,
        temp: &Path,
        since: i64,
        prev_last_visit_id: i64,
        cancelled: CancelProbe,
    ) -> AppResult<Self> {
        Self::prepare_with_schema(
            source_path,
            temp,
            None,
            since,
            prev_last_visit_id,
            cancelled,
            MAX_SPOOL_BYTES,
            Duration::from_secs(300),
        )
    }

    #[cfg(test)]
    fn prepare_limited(
        source_path: &Path,
        temp: &Path,
        is_firefox: bool,
        since: i64,
        cancelled: CancelProbe,
        max_bytes: u64,
        timeout: Duration,
    ) -> AppResult<Self> {
        Self::prepare_with_schema(
            source_path,
            temp,
            Some(is_firefox),
            since,
            0,
            cancelled,
            max_bytes,
            timeout,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn prepare_with_schema(
        source_path: &Path,
        temp: &Path,
        is_firefox: Option<bool>,
        since: i64,
        prev_last_visit_id: i64,
        cancelled: CancelProbe,
        max_bytes: u64,
        timeout: Duration,
    ) -> AppResult<Self> {
        let deadline = Instant::now() + timeout;
        let source = open_spool_source(source_path, temp, cancelled.clone(), deadline)?;
        let is_firefox = match is_firefox {
            Some(flag) => flag,
            None => super::snapshot::detect_history_schema_from_conn(&source)?,
        };
        let mut effective_since = since;
        let mut cursor_regressed = false;
        if effective_since > 0 && prev_last_visit_id > 0 {
            let max_id_sql = if is_firefox {
                "SELECT COALESCE(MAX(id), 0) FROM moz_historyvisits"
            } else {
                "SELECT COALESCE(MAX(id), 0) FROM visits"
            };
            if let Ok(source_max_id) = source.query_row(max_id_sql, [], |r| r.get::<_, i64>(0)) {
                if source_max_id < prev_last_visit_id {
                    tracing::info!(
                        source_max_id,
                        prev_last_visit_id,
                        "Large source visit ID regressed; switching spool to safe rescan and cursor reset"
                    );
                    effective_since = 0;
                    cursor_regressed = true;
                }
            }
        }
        let size = logical_size(&source)?;
        let root = temp
            .parent()
            .ok_or_else(|| AppError::Security("应用目录缺失".into()))?;
        require_available_space(root, protected_snapshot_requirement(size)?)?;
        let integrity: String = source.query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?;
        if integrity != "ok" {
            return Err(AppError::Security("大库源文件完整性检查失败".into()));
        }
        check(&cancelled, deadline)?;
        let workspace = Workspace::create(temp)?;
        let key = MasterKey::generate();
        let mut target = open_encrypted_database(&workspace.db(), key.as_bytes(), true)?;
        target.pragma_update(None, "cache_size", -8192)?;
        let _: u64 = target.query_row("PRAGMA mmap_size=0", [], |r| r.get(0))?;
        // Keep headroom for one small WAL batch, checkpoint overlap and SHM.
        // The quota covers the actual DB + WAL + SHM/journal, not just pages.
        let reserve = (max_bytes / 3 * 2).min(64 * 1024 * 1024);
        let pages = ((max_bytes - reserve) / 4096).max(1);
        let limit_sql = format!("PRAGMA max_page_count={pages}");
        let _: u64 = target.query_row(&limit_sql, [], |r| r.get(0))?;
        let probe = cancelled.clone();
        target.progress_handler(10_000, Some(move || probe() || Instant::now() >= deadline))?;
        target.execute_batch("CREATE TABLE records(ordinal INTEGER PRIMARY KEY,visit_id INTEGER NOT NULL,time_ms INTEGER NOT NULL,from_visit INTEGER NOT NULL,transition INTEGER NOT NULL,duration INTEGER NOT NULL,url TEXT NOT NULL,title TEXT NOT NULL)")?;
        let mut count = 0u32;
        let mut chunk_rows = 0u32;
        let mut chunk_bytes = 0u64;
        let mut expected = Sha256::new();
        let mut peak_bytes = workspace.enforce_quota(max_bytes)?;
        {
            target.execute_batch("BEGIN IMMEDIATE")?;
            let mut insert =
                target.prepare("INSERT INTO records VALUES(?1,?2,?3,?4,?5,?6,?7,?8)")?;
            let mut consume = |record: RawVisitRecord| -> AppResult<()> {
                check(&cancelled, deadline)?;
                count = count
                    .checked_add(1)
                    .ok_or_else(|| AppError::Security("导入记录数量溢出".into()))?;
                insert.execute(params![
                    count,
                    record.source_visit_id,
                    record.visit_time_unix_ms,
                    record.from_visit,
                    record.transition,
                    record.visit_duration,
                    record.url,
                    record.title
                ])?;
                record_hash(&mut expected, &record);
                chunk_rows += 1;
                chunk_bytes = chunk_bytes.saturating_add(
                    (record.url.len() + record.title.len() + std::mem::size_of::<RawVisitRecord>())
                        as u64,
                );
                // Bound WAL payload while amortizing checkpoint cost for the
                // common case of many short browser records.
                if chunk_rows >= 1024 || chunk_bytes >= 4 * 1024 * 1024 {
                    check(&cancelled, deadline)?;
                    target.execute_batch("COMMIT")?;
                    peak_bytes = peak_bytes.max(workspace.enforce_quota(max_bytes)?);
                    let busy: i64 =
                        target.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))?;
                    if busy != 0 {
                        return Err(AppError::Security("加密暂存分块检查点未完成".into()));
                    }
                    peak_bytes = peak_bytes.max(workspace.enforce_quota(max_bytes)?);
                    require_available_space(root, 64 * 1024 * 1024)?;
                    target.execute_batch("BEGIN IMMEDIATE")?;
                    chunk_rows = 0;
                    chunk_bytes = 0;
                }
                Ok(())
            };
            if is_firefox {
                crate::browser::firefox::for_each_firefox_record(
                    &source,
                    effective_since,
                    false,
                    &mut consume,
                )?;
            } else {
                super::parser::for_each_chromium_record(
                    &source,
                    effective_since,
                    false,
                    &mut consume,
                )?;
            }
            drop(insert);
            check(&cancelled, deadline)?;
            target.execute_batch("COMMIT")?;
            peak_bytes = peak_bytes.max(workspace.enforce_quota(max_bytes)?);
        }
        source.finish()?;
        let busy: i64 = target.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))?;
        if busy != 0 {
            return Err(AppError::Security("加密记录暂存 WAL 无法合并".into()));
        }
        peak_bytes = peak_bytes.max(workspace.enforce_quota(max_bytes)?);
        drop(target);
        let mut reopened = open_encrypted_database(&workspace.db(), key.as_bytes(), false)?;
        let probe = cancelled.clone();
        reopened.progress_handler(10_000, Some(move || probe() || Instant::now() >= deadline))?;
        if !check_encrypted_integrity(&reopened)? {
            return Err(AppError::Security("加密记录暂存校验失败".into()));
        }
        reopened.pragma_update(None, "query_only", true)?;
        let mut result = Self {
            connection: reopened,
            _key: key,
            _workspace: workspace,
            count,
            cursor_regressed,
            #[cfg(test)]
            peak_bytes,
            cancelled,
        };
        let mut actual = Sha256::new();
        let mut read_count = 0u32;
        result.for_each(|record| {
            check(&result.cancelled, deadline)?;
            record_hash(&mut actual, &record);
            read_count += 1;
            Ok(())
        })?;
        if read_count != count || actual.finalize() != expected.finalize() {
            return Err(AppError::Security("加密记录暂存内容与源记录不一致".into()));
        }
        result._workspace.enforce_quota(max_bytes)?;
        tracing::debug!(peak_bytes, "Encrypted record spool verified");
        // Preparation timeout must not later interrupt a valid commit. Session
        // cancellation is still checked for every record and SQL progress step.
        let probe = result.cancelled.clone();
        result
            .connection
            .progress_handler(10_000, Some(move || probe()))?;
        Ok(result)
    }

    pub(crate) fn for_each<F>(&self, mut consume: F) -> AppResult<()>
    where
        F: FnMut(RawVisitRecord) -> AppResult<()>,
    {
        let mut statement=self.connection.prepare("SELECT visit_id,time_ms,from_visit,transition,duration,url,title FROM records ORDER BY ordinal")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            if (self.cancelled)() {
                return Err(AppError::VaultLocked(
                    "保险库会话已结束，已取消大库提交".into(),
                ));
            }
            consume(RawVisitRecord {
                source_visit_id: row.get(0)?,
                visit_time_unix_ms: row.get(1)?,
                from_visit: row.get(2)?,
                transition: row.get(3)?,
                visit_duration: row.get(4)?,
                url: limited_text(row, 5, 64 * 1024)?,
                title: limited_text(row, 6, 256 * 1024)?,
            })?;
        }
        Ok(())
    }
    pub(crate) fn len(&self) -> u32 {
        self.count
    }

    pub(crate) fn cursor_regressed(&self) -> bool {
        self.cursor_regressed
    }
}

/// Startup-only sweep after acquiring the application's directory ownership.
pub(crate) fn cleanup_orphans(
    root: &Path,
    _ownership: &crate::security::directory_lock::DataDirectoryLock,
) -> usize {
    let Ok(Some(base)) = resolve_scoped_path(root, Path::new(WORK)) else {
        return 0;
    };
    let Ok(entries) = fs::read_dir(base) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.take(1024).flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !uuid::Uuid::parse_str(name)
            .map(|id| id.to_string() == name)
            .unwrap_or(false)
        {
            continue;
        }
        if remove_owned_workspace(root, &Path::new(WORK).join(name)).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn source_fixture(root: &Path, firefox: bool) -> (PathBuf, Connection) {
        fs::create_dir_all(root).unwrap();
        let path = root.join("source_'_历史.db");
        let conn = Connection::open(&path).unwrap();
        conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get::<_, String>(0))
            .unwrap();
        conn.query_row("PRAGMA wal_autocheckpoint=0", [], |r| r.get::<_, i64>(0))
            .unwrap();
        if firefox {
            conn.execute_batch("CREATE TABLE moz_places(id INTEGER PRIMARY KEY,url TEXT,title TEXT); CREATE TABLE moz_historyvisits(id INTEGER PRIMARY KEY,place_id INTEGER,visit_date INTEGER,from_visit INTEGER,visit_type INTEGER); INSERT INTO moz_places VALUES(1,'https://private-spool.test/PRIVATE-SPOOL-SENTINEL','PRIVATE-SPOOL-SENTINEL'); INSERT INTO moz_historyvisits VALUES(1,1,1700000000000000,0,1),(2,1,1700000000001000,0,1);").unwrap();
        } else {
            conn.execute_batch("CREATE TABLE urls(id INTEGER PRIMARY KEY,url TEXT,title TEXT); CREATE TABLE visits(id INTEGER PRIMARY KEY,url INTEGER,visit_time INTEGER,from_visit INTEGER,transition INTEGER,visit_duration INTEGER); INSERT INTO urls VALUES(1,'https://private-spool.test/PRIVATE-SPOOL-SENTINEL','PRIVATE-SPOOL-SENTINEL'); INSERT INTO visits VALUES(1,1,13344473600000000,0,1,0),(2,1,13344473600001000,0,1,0);").unwrap();
        }
        (path, conn)
    }

    #[test]
    fn encrypted_spool_is_reopened_verified_stable_and_removed_without_plaintext() {
        for firefox in [false, true] {
            let root =
                std::env::temp_dir().join(format!("browsory_spool_{}", uuid::Uuid::new_v4()));
            let app = root.join("app");
            fs::create_dir_all(&app).unwrap();
            let (source, writer) = source_fixture(&root.join("browser"), firefox);
            let db_before = fs::read(&source).unwrap();
            let wal = source.with_file_name(format!(
                "{}-wal",
                source.file_name().unwrap().to_string_lossy()
            ));
            let wal_before = fs::read(&wal).unwrap();
            let spool =
                RecordSpool::prepare(&source, &app.join("temp"), firefox, 0, Arc::new(|| false))
                    .unwrap();
            assert_eq!(spool.len(), 2);
            assert!(!app.join("temp").exists());
            assert_eq!(fs::read(&source).unwrap(), db_before);
            assert_eq!(fs::read(&wal).unwrap(), wal_before);
            let path = spool._workspace.db();
            for entry in fs::read_dir(path.parent().unwrap()).unwrap() {
                let bytes = fs::read(entry.unwrap().path()).unwrap();
                assert!(!bytes
                    .windows(b"PRIVATE-SPOOL-SENTINEL".len())
                    .any(|part| part == b"PRIVATE-SPOOL-SENTINEL"));
            }
            assert!(open_encrypted_database(&path, &[0; 32], false).is_err());
            let plain =
                Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
            assert!(plain
                .query_row("SELECT count(*) FROM records", [], |r| r.get::<_, i64>(0))
                .is_err());
            drop(plain);
            writer
                .execute(
                    if firefox {
                        "UPDATE moz_places SET title='later write'"
                    } else {
                        "UPDATE urls SET title='later write'"
                    },
                    [],
                )
                .unwrap();
            let mut count = 0;
            spool
                .for_each(|record| {
                    assert_eq!(record.title, "PRIVATE-SPOOL-SENTINEL");
                    count += 1;
                    Ok(())
                })
                .unwrap();
            assert_eq!(count, 2);
            drop(spool);
            assert!(!path.exists());
            drop(writer);
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn chunk_checkpoints_bound_combined_database_and_wal_bytes() {
        let root =
            std::env::temp_dir().join(format!("browsory_spool_quota_{}", uuid::Uuid::new_v4()));
        let app = root.join("app");
        fs::create_dir_all(&app).unwrap();
        let (source, writer) = source_fixture(&root.join("browser"), false);
        writer
            .execute("UPDATE urls SET title=?1", ["q".repeat(8 * 1024)])
            .unwrap();
        writer.execute_batch("WITH RECURSIVE n(i) AS(SELECT 3 UNION ALL SELECT i+1 FROM n WHERE i<100) INSERT INTO visits SELECT i,1,13344473600000000+i,0,1,0 FROM n;").unwrap();
        let limit = 4 * 1024 * 1024;
        let spool = RecordSpool::prepare_limited(
            &source,
            &app.join("temp"),
            false,
            0,
            Arc::new(|| false),
            limit,
            Duration::from_secs(30),
        )
        .unwrap();
        assert_eq!(spool.len(), 100);
        assert!(spool.peak_bytes <= limit);
        assert!(spool.peak_bytes > 500 * 1024);
        assert!(spool._workspace.enforce_quota(limit).unwrap() <= limit);
        drop(spool);
        assert_eq!(fs::read_dir(app.join(WORK)).unwrap().count(), 0);
        drop(writer);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn timeout_cancellation_and_sqlite_full_leave_no_spool_or_changed_source() {
        let root =
            std::env::temp_dir().join(format!("browsory_spool_fail_{}", uuid::Uuid::new_v4()));
        let app = root.join("app");
        fs::create_dir_all(&app).unwrap();
        let (source, writer) = source_fixture(&root.join("browser"), false);
        let before = fs::read(&source).unwrap();
        assert!(RecordSpool::prepare_limited(
            &source,
            &app.join("temp"),
            false,
            0,
            Arc::new(|| false),
            MAX_SPOOL_BYTES,
            Duration::ZERO
        )
        .is_err());
        assert!(
            RecordSpool::prepare(&source, &app.join("temp"), false, 0, Arc::new(|| true)).is_err()
        );
        writer
            .execute("UPDATE urls SET title=?1", ["x".repeat(100 * 1024)])
            .unwrap();
        let result = RecordSpool::prepare_limited(
            &source,
            &app.join("temp"),
            false,
            0,
            Arc::new(|| false),
            3 * 4096,
            Duration::from_secs(30),
        );
        assert!(result.is_err());
        assert_eq!(fs::read(&source).unwrap(), before);
        assert!(!app.join("temp").exists());
        assert_eq!(fs::read_dir(app.join(WORK)).unwrap().count(), 0);
        drop(writer);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn real_vault_lock_cancels_spool_preparation_and_drains_registered_work() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        let root =
            std::env::temp_dir().join(format!("browsory_spool_lock_{}", uuid::Uuid::new_v4()));
        let app = root.join("app");
        fs::create_dir_all(&app).unwrap();
        let (source, writer) = source_fixture(&root.join("browser"), false);
        writer.execute_batch("WITH RECURSIVE n(i) AS(SELECT 3 UNION ALL SELECT i+1 FROM n WHERE i<1000) INSERT INTO visits SELECT i,1,13344473600000000+i,0,1,0 FROM n;").unwrap();
        let vault = Arc::new(crate::security::vault::VaultManager::unlocked_fixture(
            app.clone(),
        ));
        let operation = vault.begin_operation().unwrap();
        let cancelled = operation.cancellation_probe();
        let calls = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(AtomicBool::new(false));
        let locker = Arc::new(std::sync::Mutex::new(None));
        let handle = locker.clone();
        let closing = vault.clone();
        let probe = Arc::new(move || {
            if calls.fetch_add(1, Ordering::AcqRel) > 10 && !started.swap(true, Ordering::AcqRel) {
                let closing_thread = closing.clone();
                *handle.lock().unwrap() =
                    Some(std::thread::spawn(move || closing_thread.lock().unwrap()));
                let deadline = Instant::now() + Duration::from_secs(5);
                while closing.status() != crate::security::vault::VaultStatus::Locking {
                    assert!(Instant::now() < deadline);
                    std::thread::yield_now();
                }
            }
            cancelled()
        });
        assert!(RecordSpool::prepare(&source, &app.join("temp"), false, 0, probe).is_err());
        assert!(operation.check().is_err());
        drop(operation);
        locker.lock().unwrap().take().unwrap().join().unwrap();
        assert_eq!(vault.status(), crate::security::vault::VaultStatus::Locked);
        assert!(vault.master_key().is_none());
        if app.join(WORK).exists() {
            assert_eq!(fs::read_dir(app.join(WORK)).unwrap().count(), 0);
        }
        drop(vault);
        drop(writer);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn crash_probe() {
        let Some(root) = std::env::var_os("BROWSORY_SPOOL_CRASH_ROOT") else {
            return;
        };
        let root = PathBuf::from(root);
        let app = root.join("app");
        let _owner = crate::security::directory_lock::DataDirectoryLock::acquire(&app).unwrap();
        let (source, _writer) = source_fixture(&root.join("browser"), false);
        let _spool =
            RecordSpool::prepare(&source, &app.join("temp"), false, 0, Arc::new(|| false)).unwrap();
        fs::write(root.join("ready"), b"ready").unwrap();
        std::thread::sleep(Duration::from_secs(30));
    }

    #[test]
    fn forced_process_exit_leaves_ciphertext_only_and_owned_startup_cleans_it() {
        use std::process::Command;
        let root =
            std::env::temp_dir().join(format!("browsory_spool_crash_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("app")).unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "history::record_spool::tests::crash_probe",
                "--test-threads=1",
            ])
            .env("BROWSORY_SPOOL_CRASH_ROOT", &root);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !root.join("ready").exists() {
            if Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("spool child did not become ready");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            crate::security::directory_lock::DataDirectoryLock::acquire(&root.join("app")).is_err()
        );
        child.kill().unwrap();
        child.wait().unwrap();
        let app = root.join("app");
        let directory = fs::read_dir(app.join(WORK))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let bytes = fs::read(directory.join(DB)).unwrap();
        assert!(!bytes.starts_with(b"SQLite format 3"));
        assert!(!bytes
            .windows(b"PRIVATE-SPOOL-SENTINEL".len())
            .any(|part| part == b"PRIVATE-SPOOL-SENTINEL"));
        assert!(!directory.join("vault.envelope.json").exists());
        let owner = crate::security::directory_lock::DataDirectoryLock::acquire(&app).unwrap();
        assert_eq!(cleanup_orphans(&app, &owner), 1);
        assert!(!directory.exists());
        drop(owner);
        fs::remove_dir_all(root).unwrap();
    }
}
