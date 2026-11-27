use rusqlite::backup::{Backup, StepResult};
use rusqlite::{Connection, ErrorCode, OpenFlags};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime};
use tracing::{debug, warn};
use uuid::Uuid;

use crate::error::{AppError, AppResult};

// Limits cover SQLite snapshot pages, not parsed records or total process RAM.
const MAX_SNAPSHOT_BYTES: u64 = 256 * 1024 * 1024;
const MAX_TOTAL_SNAPSHOT_BYTES: u64 = 512 * 1024 * 1024;
static SNAPSHOT_BYTES: AtomicU64 = AtomicU64::new(0);
const SQLITE_HEADER_MAGIC: &[u8; 16] = b"SQLite format 3\0";
const ROLLBACK_JOURNAL_MAGIC: [u8; 8] = [0xd9, 0xd5, 0x05, 0xf9, 0x20, 0xa1, 0x63, 0xd7];

struct SnapshotBudget {
    bytes: u64,
}
impl SnapshotBudget {
    fn reserve(bytes: u64) -> AppResult<Self> {
        SNAPSHOT_BYTES
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes)
                    .filter(|total| *total <= MAX_TOTAL_SNAPSHOT_BYTES)
            })
            .map_err(|_| {
                AppError::Security(
                    "内存快照总预算已用尽，请稍后重试；不会回退到明文磁盘副本".into(),
                )
            })?;
        Ok(Self { bytes })
    }

    fn resize(&mut self, new_bytes: u64) -> AppResult<()> {
        if new_bytes <= self.bytes {
            let delta = self.bytes - new_bytes;
            if delta > 0 {
                SNAPSHOT_BYTES.fetch_sub(delta, Ordering::AcqRel);
                self.bytes = new_bytes;
            }
            return Ok(());
        }
        let extra = new_bytes - self.bytes;
        SNAPSHOT_BYTES
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(extra)
                    .filter(|total| *total <= MAX_TOTAL_SNAPSHOT_BYTES)
            })
            .map_err(|_| {
                AppError::Security(
                    "内存快照总预算已用尽，请稍后重试；不会回退到明文磁盘副本".into(),
                )
            })?;
        self.bytes = new_bytes;
        Ok(())
    }
}
impl Drop for SnapshotBudget {
    fn drop(&mut self) {
        SNAPSHOT_BYTES.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

pub struct SnapshotGuard {
    // Fields drop in declaration order: release SQLite pages before budget.
    connection: Connection,
    _budget: SnapshotBudget,
}
impl SnapshotGuard {
    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    pub fn connection_mut(&mut self) -> &mut Connection {
        &mut self.connection
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

/// Read a transactionally consistent SQLite snapshot directly into memory.
/// Never materialize a plaintext copy in the app directory or fall back to one.
pub fn create_history_snapshot(
    source_history_file: &Path,
    _temp_base_dir: &Path,
) -> AppResult<SnapshotGuard> {
    create_snapshot_with_limits_checked(
        source_history_file,
        MAX_SNAPSHOT_BYTES,
        Duration::from_secs(10),
        &|| false,
    )
}

pub fn create_history_snapshot_checked(
    source_history_file: &Path,
    _temp_base_dir: &Path,
    cancelled: &dyn Fn() -> bool,
) -> AppResult<SnapshotGuard> {
    create_snapshot_with_limits_checked(
        source_history_file,
        MAX_SNAPSHOT_BYTES,
        Duration::from_secs(10),
        cancelled,
    )
}

pub(crate) fn is_sqlite_locked_error(err: &rusqlite::Error) -> bool {
    match err {
        rusqlite::Error::SqliteFailure(ffi_err, msg) => {
            matches!(
                ffi_err.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            ) || msg
                .as_deref()
                .map(|m| m.to_ascii_lowercase())
                .is_some_and(|m| m.contains("database is locked") || m.contains("database is busy"))
        }
        _ => false,
    }
}

pub(crate) fn is_app_error_locked(err: &AppError) -> bool {
    matches!(err, AppError::Database(db_err) if is_sqlite_locked_error(db_err))
}

pub(crate) fn detect_history_schema_from_conn(conn: &Connection) -> AppResult<bool> {
    let tables: (i64, i64) = conn.query_row(
        "SELECT sum(name IN ('moz_historyvisits','moz_places')), sum(name IN ('visits','urls')) FROM sqlite_master WHERE type='table'",
        [],
        |r| {
            Ok((
                r.get::<_, Option<i64>>(0)?.unwrap_or(0),
                r.get::<_, Option<i64>>(1)?.unwrap_or(0),
            ))
        },
    )?;
    match tables {
        (2, _) => Ok(true),
        (_, 2) => Ok(false),
        _ => Err(AppError::Parse("不支持的浏览器历史数据库结构".into())),
    }
}

pub fn probe_supported_history_db(source_file: &Path) -> bool {
    if let Ok(conn) = Connection::open_with_flags(
        source_file,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        if crate::database::sqlcipher::configure_private_connection(&conn).is_ok() {
            match detect_history_schema_from_conn(&conn) {
                Ok(_) => return true,
                Err(ref err) if !is_app_error_locked(err) => return false,
                Err(_) => {}
            }
        }
    }

    if let Ok(guard) =
        create_snapshot_with_limits(source_file, MAX_SNAPSHOT_BYTES, Duration::from_secs(3))
    {
        return detect_history_schema_from_conn(guard.connection()).is_ok();
    }

    #[cfg(windows)]
    {
        if fs::metadata(source_file)
            .map(|m| m.len() > MAX_SNAPSHOT_BYTES)
            .unwrap_or(false)
            && !has_active_rollback_journal(&sidecar_path(source_file, "-journal")).unwrap_or(true)
            && !has_committed_wal_frames(&sidecar_path(source_file, "-wal")).unwrap_or(true)
        {
            if let Ok(conn) = Connection::open_with_flags_and_vfs(
                source_file,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
                "win32-none",
            ) {
                if crate::database::sqlcipher::configure_private_connection(&conn).is_ok() {
                    return detect_history_schema_from_conn(&conn).is_ok();
                }
            }
        }
    }

    false
}

pub(crate) fn sidecar_path(source_file: &Path, suffix: &str) -> PathBuf {
    let mut os = source_file.as_os_str().to_os_string();
    os.push(suffix);
    PathBuf::from(os)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileStamp {
    pub exists: bool,
    pub len: u64,
    pub modified: Option<SystemTime>,
}

pub(crate) fn read_file_stamp(path: &Path) -> AppResult<FileStamp> {
    match fs::metadata(path) {
        Ok(meta) => Ok(FileStamp {
            exists: true,
            len: meta.len(),
            modified: meta.modified().ok(),
        }),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(FileStamp {
            exists: false,
            len: 0,
            modified: None,
        }),
        Err(err) => Err(err.into()),
    }
}

pub(crate) fn has_active_rollback_journal(journal_path: &Path) -> AppResult<bool> {
    let mut file = match fs::File::open(journal_path) {
        Ok(f) => f,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err.into()),
    };
    let mut header = [0u8; 8];
    let n = file.read(&mut header)?;
    Ok(n == 8 && header == ROLLBACK_JOURNAL_MAGIC)
}

pub(crate) fn has_committed_wal_frames(wal_path: &Path) -> AppResult<bool> {
    let mut file = match fs::File::open(wal_path) {
        Ok(f) => f,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err.into()),
    };
    let mut header = [0u8; 32];
    if file.read(&mut header)? < 32 {
        return Ok(false);
    }
    let magic = u32::from_be_bytes(header[0..4].try_into().unwrap());
    Ok(magic == 0x377f_0682 || magic == 0x377f_0683)
}

fn read_bounded_bytes(path: &Path, max_bytes: u64) -> AppResult<Option<Vec<u8>>> {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let meta = file.metadata()?;
    if meta.len() > max_bytes {
        return Err(AppError::Security(
            "历史库超过内存快照限制（默认 256 MiB）或为空，已中止；不会创建明文磁盘副本".into(),
        ));
    }
    let mut buf = Vec::with_capacity(meta.len() as usize);
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut buf)?;
    if buf.len() as u64 > max_bytes {
        return Err(AppError::Security(
            "历史库超过内存快照限制（默认 256 MiB）或为空，已中止；不会创建明文磁盘副本".into(),
        ));
    }
    Ok(Some(buf))
}

fn wal_checksum_bytes(big_endian: bool, data: &[u8], mut s0: u32, mut s1: u32) -> (u32, u32) {
    for chunk in data.chunks_exact(8) {
        let (x0, x1) = if big_endian {
            (
                u32::from_be_bytes(chunk[0..4].try_into().unwrap()),
                u32::from_be_bytes(chunk[4..8].try_into().unwrap()),
            )
        } else {
            (
                u32::from_le_bytes(chunk[0..4].try_into().unwrap()),
                u32::from_le_bytes(chunk[4..8].try_into().unwrap()),
            )
        };
        s0 = s0.wrapping_add(x0).wrapping_add(s1);
        s1 = s1.wrapping_add(x1).wrapping_add(s0);
    }
    (s0, s1)
}

fn apply_committed_wal_frames(
    db_bytes: &mut Vec<u8>,
    wal: &[u8],
    page_size: usize,
    max_bytes: u64,
    budget: &mut SnapshotBudget,
) -> AppResult<()> {
    if wal.len() < 32 {
        return Ok(());
    }
    let magic = u32::from_be_bytes(wal[0..4].try_into().unwrap());
    let big_endian = match magic {
        0x377f_0682 => false,
        0x377f_0683 => true,
        _ => return Ok(()),
    };
    let format = u32::from_be_bytes(wal[4..8].try_into().unwrap());
    let wal_page_size = u32::from_be_bytes(wal[8..12].try_into().unwrap()) as usize;
    if format != 3_007_000 || wal_page_size != page_size {
        return Ok(());
    }
    let hdr_cksum0 = u32::from_be_bytes(wal[24..28].try_into().unwrap());
    let hdr_cksum1 = u32::from_be_bytes(wal[28..32].try_into().unwrap());
    let mut cksum = wal_checksum_bytes(big_endian, &wal[0..24], 0, 0);
    if cksum != (hdr_cksum0, hdr_cksum1) {
        return Ok(());
    }
    let wal_salt = &wal[16..24];
    let frame_size = 24 + page_size;
    let mut committed: HashMap<u32, &[u8]> = HashMap::new();
    let mut pending: Vec<(u32, &[u8])> = Vec::new();
    let mut last_commit_db_size: Option<u32> = None;

    for frame in wal[32..].chunks_exact(frame_size) {
        let pgno = u32::from_be_bytes(frame[0..4].try_into().unwrap());
        if pgno == 0 || &frame[8..16] != wal_salt {
            break;
        }
        let commit_db_size = u32::from_be_bytes(frame[4..8].try_into().unwrap());
        let expected0 = u32::from_be_bytes(frame[16..20].try_into().unwrap());
        let expected1 = u32::from_be_bytes(frame[20..24].try_into().unwrap());
        let (s0, s1) = wal_checksum_bytes(big_endian, &frame[0..8], cksum.0, cksum.1);
        let (s0, s1) = wal_checksum_bytes(big_endian, &frame[24..24 + page_size], s0, s1);
        if (s0, s1) != (expected0, expected1) {
            break;
        }
        cksum = (s0, s1);
        pending.push((pgno, &frame[24..24 + page_size]));
        if commit_db_size > 0 {
            for (committed_pgno, page_slice) in pending.drain(..) {
                committed.insert(committed_pgno, page_slice);
            }
            last_commit_db_size = Some(commit_db_size);
        }
    }

    if let Some(commit_pages) = last_commit_db_size {
        let target_len = (commit_pages as u64)
            .checked_mul(page_size as u64)
            .ok_or_else(|| AppError::Security("历史库大小溢出".into()))?;
        if target_len == 0 || target_len > max_bytes {
            return Err(AppError::Security(
                "历史库超过内存快照限制（默认 256 MiB）或为空，已中止；不会创建明文磁盘副本".into(),
            ));
        }
        budget.resize(target_len)?;
        db_bytes.resize(target_len as usize, 0);
        for (pgno, page_slice) in committed {
            if pgno >= 1 && pgno <= commit_pages {
                let offset = (pgno as usize - 1) * page_size;
                db_bytes[offset..offset + page_size].copy_from_slice(page_slice);
            }
        }
    }

    Ok(())
}

fn try_sqlite_backup_snapshot(
    source_file: &Path,
    max_bytes: u64,
    timeout: Duration,
    cancelled: &dyn Fn() -> bool,
) -> AppResult<SnapshotGuard> {
    if cancelled() {
        return Err(AppError::VaultLocked("保险库会话已结束".into()));
    }
    let source = Connection::open_with_flags(
        source_file,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    crate::database::sqlcipher::configure_private_connection(&source)?;
    source.busy_timeout(Duration::from_millis(100))?;
    // Pin the source read transaction before sizing it, so a concurrent writer
    // cannot grow the snapshot beyond the reserved page budget between steps.
    source.execute_batch("BEGIN DEFERRED")?;
    let pages: u64 = source.pragma_query_value(None, "page_count", |r| r.get(0))?;
    let page_size: u64 = source.pragma_query_value(None, "page_size", |r| r.get(0))?;
    let bytes = pages
        .checked_mul(page_size)
        .ok_or_else(|| AppError::Security("历史库大小溢出".into()))?;
    if bytes == 0 || bytes > max_bytes {
        return Err(AppError::Security(
            "历史库超过内存快照限制（默认 256 MiB）或为空，已中止；不会创建明文磁盘副本".into(),
        ));
    }
    let budget = SnapshotBudget::reserve(bytes)?;
    let mut snapshot = Connection::open_in_memory()?;
    crate::database::sqlcipher::configure_private_connection(&snapshot)?;
    snapshot.pragma_update(None, "page_size", page_size)?;
    let deadline = Instant::now() + timeout;
    let mut busy_streak = 0u32;
    {
        let backup = Backup::new(&source, &mut snapshot)?;
        loop {
            if cancelled() {
                return Err(AppError::VaultLocked("保险库会话已结束".into()));
            }
            if Instant::now() >= deadline {
                return Err(AppError::Browser("内存快照超时，已中止；请稍后重试".into()));
            }
            match backup.step(256)? {
                StepResult::Done => break,
                StepResult::More => busy_streak = 0,
                StepResult::Busy | StepResult::Locked => {
                    busy_streak += 1;
                    if busy_streak >= 30 {
                        return Err(AppError::Database(rusqlite::Error::SqliteFailure(
                            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
                            Some("database is locked".into()),
                        )));
                    }
                    sleep(Duration::from_millis(10));
                }
                _ => return Err(AppError::Browser("内存快照返回未知状态，已中止".into())),
            }
        }
    }
    // Close the browser read transaction before parsing or committing records.
    source.execute_batch("ROLLBACK")?;
    let actual_pages: u64 = snapshot.pragma_query_value(None, "page_count", |r| r.get(0))?;
    if actual_pages
        .checked_mul(page_size)
        .is_none_or(|size| size > bytes)
    {
        return Err(AppError::Security("快照超出预留预算，已中止".into()));
    }
    let check: String = snapshot.query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?;
    if check != "ok" {
        return Err(AppError::Security("内存快照完整性检查失败".into()));
    }
    snapshot.pragma_update(None, "query_only", true)?;
    debug!("Created verified in-memory history snapshot via SQLite Backup API");
    Ok(SnapshotGuard {
        connection: snapshot,
        _budget: budget,
    })
}

fn create_lock_free_memory_snapshot(
    source_file: &Path,
    max_bytes: u64,
    timeout: Duration,
    cancelled: &dyn Fn() -> bool,
) -> AppResult<SnapshotGuard> {
    let journal_path = sidecar_path(source_file, "-journal");
    let wal_path = sidecar_path(source_file, "-wal");
    let deadline = Instant::now() + timeout;

    loop {
        if cancelled() {
            return Err(AppError::VaultLocked("保险库会话已结束".into()));
        }
        if has_active_rollback_journal(&journal_path)? {
            if Instant::now() >= deadline {
                return Err(AppError::Browser(
                    "浏览器历史库正在写入事务中，请稍后重试".into(),
                ));
            }
            sleep(Duration::from_millis(25));
            continue;
        }

        let pre_db = read_file_stamp(source_file)?;
        if !pre_db.exists || pre_db.len < 100 || pre_db.len > max_bytes {
            return Err(AppError::Security(
                "历史库超过内存快照限制（默认 256 MiB）或为空，已中止；不会创建明文磁盘副本".into(),
            ));
        }
        let pre_journal = read_file_stamp(&journal_path)?;
        let pre_wal = read_file_stamp(&wal_path)?;

        let mut budget = SnapshotBudget::reserve(pre_db.len)?;
        let mut db_bytes = read_bounded_bytes(source_file, max_bytes)?
            .ok_or_else(|| AppError::Browser("未找到浏览器历史库文件".into()))?;
        let wal_bytes = if pre_wal.exists && pre_wal.len > 0 {
            read_bounded_bytes(&wal_path, max_bytes)?
        } else {
            None
        };

        let post_db = read_file_stamp(source_file)?;
        let post_journal = read_file_stamp(&journal_path)?;
        let post_wal = read_file_stamp(&wal_path)?;
        if pre_db != post_db
            || pre_journal != post_journal
            || pre_wal != post_wal
            || has_active_rollback_journal(&journal_path)?
        {
            if cancelled() {
                return Err(AppError::VaultLocked("保险库会话已结束".into()));
            }
            if Instant::now() >= deadline {
                return Err(AppError::Browser(
                    "读取浏览器历史库时检测到并发写入，请稍后重试".into(),
                ));
            }
            sleep(Duration::from_millis(25));
            continue;
        }

        if db_bytes.len() < 100 || &db_bytes[0..16] != SQLITE_HEADER_MAGIC {
            return Err(AppError::Parse("不支持或已损坏的浏览器历史数据库头".into()));
        }
        let raw_page_size = u16::from_be_bytes([db_bytes[16], db_bytes[17]]) as usize;
        let page_size = if raw_page_size == 1 {
            65_536
        } else {
            raw_page_size
        };
        if !page_size.is_power_of_two() || !(512..=65_536).contains(&page_size) {
            return Err(AppError::Parse("浏览器历史数据库页大小无效".into()));
        }

        if let Some(wal) = wal_bytes.as_deref() {
            apply_committed_wal_frames(&mut db_bytes, wal, page_size, max_bytes, &mut budget)?;
        }

        if db_bytes[18] == 2 || db_bytes[19] == 2 {
            // In-memory VFS has no SHM; WAL frames are already applied above.
            db_bytes[18] = 1;
            db_bytes[19] = 1;
        }

        let db_len = db_bytes.len();
        let mut snapshot = Connection::open_in_memory()?;
        crate::database::sqlcipher::configure_private_connection(&snapshot)?;
        snapshot.deserialize_read_exact(
            rusqlite::MAIN_DB,
            Cursor::new(&db_bytes),
            db_len,
            false,
        )?;
        drop(db_bytes);

        let check_ok = snapshot
            .query_row("PRAGMA quick_check(1)", [], |r| r.get::<_, String>(0))
            .is_ok_and(|res| res == "ok");
        if !check_ok {
            if cancelled() {
                return Err(AppError::VaultLocked("保险库会话已结束".into()));
            }
            if Instant::now() < deadline {
                sleep(Duration::from_millis(25));
                continue;
            }
            return Err(AppError::Security("内存快照完整性检查失败".into()));
        }
        snapshot.pragma_update(None, "query_only", true)?;
        debug!("Created verified lock-free in-memory history snapshot");
        return Ok(SnapshotGuard {
            connection: snapshot,
            _budget: budget,
        });
    }
}

fn create_snapshot_with_limits(
    source_file: &Path,
    max_bytes: u64,
    timeout: Duration,
) -> AppResult<SnapshotGuard> {
    create_snapshot_with_limits_checked(source_file, max_bytes, timeout, &|| false)
}

fn create_snapshot_with_limits_checked(
    source_file: &Path,
    max_bytes: u64,
    timeout: Duration,
    cancelled: &dyn Fn() -> bool,
) -> AppResult<SnapshotGuard> {
    match try_sqlite_backup_snapshot(source_file, max_bytes, timeout, cancelled) {
        Ok(guard) => Ok(guard),
        Err(ref err) if is_app_error_locked(err) => {
            debug!(
                "Browser database is exclusively locked, falling back to lock-free memory snapshot: {:?}",
                source_file
            );
            create_lock_free_memory_snapshot(source_file, max_bytes, timeout, cancelled)
        }
        Err(err) => Err(err),
    }
}

const MAX_CLEANUP_DEPTH: usize = 64;
const MAX_CLEANUP_ENTRIES: usize = 100_000;

/// Scan one candidate snapshot tree without following links.
/// `Ok(None)` means the candidate must be preserved (for example, it contains
/// a link or an object that is not a regular file/directory).
fn scan_snapshot_tree(path: &Path, depth: usize, entries: &mut usize) -> AppResult<Option<u64>> {
    if depth > MAX_CLEANUP_DEPTH {
        return Ok(None);
    }
    *entries = entries
        .checked_add(1)
        .ok_or_else(|| AppError::Security("临时目录项目数量溢出".into()))?;
    if *entries > MAX_CLEANUP_ENTRIES {
        return Ok(None);
    }
    let Some(metadata) = crate::security::fs_boundary::plain_metadata(path)? else {
        return Ok(None);
    };
    if metadata.is_file() {
        return Ok(Some(metadata.len()));
    }
    if !metadata.is_dir() {
        return Ok(None);
    }

    let mut bytes = 0u64;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let Some(sub_bytes) = scan_snapshot_tree(&entry.path(), depth + 1, entries)? else {
            return Ok(None);
        };
        bytes = bytes
            .checked_add(sub_bytes)
            .ok_or_else(|| AppError::Security("临时目录大小溢出".into()))?;
    }
    Ok(Some(bytes))
}

/// Remove only UUID-named snapshot directories below `app_dir/temp`.
/// Unknown top-level objects and any candidate containing an unsafe object are
/// preserved.  The function is intentionally non-throwing for cleanup UX: a
/// failed safety check simply leaves the candidate in place and logs it.
pub fn cleanup_orphan_snapshots(app_dir: &Path, temp_base_dir: &Path) -> (usize, u64) {
    let Some(app_metadata) = crate::security::fs_boundary::plain_metadata(app_dir)
        .ok()
        .flatten()
    else {
        return (0, 0);
    };
    if !app_metadata.is_dir() {
        return (0, 0);
    }
    let Ok(canonical_app) = fs::canonicalize(app_dir) else {
        return (0, 0);
    };
    if canonical_app.parent().is_none()
        || canonical_app
            .parent()
            .is_some_and(|parent| parent == canonical_app.as_path())
    {
        warn!("拒绝将根目录作为应用目录清理: {:?}", app_dir);
        return (0, 0);
    }

    let Some(temp_metadata) = crate::security::fs_boundary::plain_metadata(temp_base_dir)
        .ok()
        .flatten()
    else {
        return (0, 0);
    };
    if !temp_metadata.is_dir() {
        return (0, 0);
    }
    let Ok(canonical_temp) = fs::canonicalize(temp_base_dir) else {
        return (0, 0);
    };
    let expected_temp = canonical_app.join("temp");
    if canonical_temp != expected_temp || canonical_temp == canonical_app {
        warn!(
            "拒绝清理越界临时目录: app={:?}, temp={:?}",
            app_dir, temp_base_dir
        );
        return (0, 0);
    }

    let mut deleted_count = 0;
    let mut bytes_freed = 0u64;
    let Ok(entries) = fs::read_dir(temp_base_dir) else {
        return (0, 0);
    };
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !Uuid::parse_str(name)
            .map(|uuid| uuid.to_string() == name)
            .unwrap_or(false)
        {
            continue;
        }
        let Ok(Some(metadata)) = crate::security::fs_boundary::plain_metadata(&path) else {
            continue;
        };
        if !metadata.is_dir() {
            continue;
        }
        let mut entry_count = 0usize;
        let Ok(Some(sub_bytes)) = scan_snapshot_tree(&path, 0, &mut entry_count) else {
            continue;
        };

        // Recheck the candidate immediately before using remove_dir_all.  The
        // canonical path must remain below app_dir/temp and cannot be temp
        // itself; links in the scanned tree are also rejected by the scan.
        let Ok(Some(recheck_metadata)) = crate::security::fs_boundary::plain_metadata(&path) else {
            continue;
        };
        if !recheck_metadata.is_dir() {
            continue;
        }
        let Ok(canonical_candidate) = fs::canonicalize(&path) else {
            continue;
        };
        if canonical_candidate == canonical_temp
            || !canonical_candidate.starts_with(&canonical_temp)
        {
            warn!("拒绝删除越界快照目录: {:?}", path);
            continue;
        }
        if fs::remove_dir_all(&path).is_ok() {
            deleted_count += 1;
            bytes_freed = bytes_freed.saturating_add(sub_bytes);
        }
    }

    if deleted_count > 0 {
        debug!(
            "清理残留孤儿快照项目: {} 项, 释放 {} 字节",
            deleted_count, bytes_freed
        );
    }

    (deleted_count, bytes_freed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_snapshot_includes_live_wal_is_stable_and_creates_no_app_files() {
        let root =
            std::env::temp_dir().join(format!("browsory_memory_snapshot_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("source.db");
        let app_temp = root.join("app").join("temp");
        let source = Connection::open(&path).unwrap();
        source.pragma_update(None, "page_size", 8192).unwrap();
        source
            .query_row("PRAGMA journal_mode=WAL", [], |r| r.get::<_, String>(0))
            .unwrap();
        source
            .query_row("PRAGMA wal_autocheckpoint=0", [], |r| r.get::<_, i64>(0))
            .unwrap();
        source
            .execute_batch(
                "CREATE TABLE t(value TEXT); INSERT INTO t VALUES('latest private WAL record');",
            )
            .unwrap();
        let before = fs::read(&path).unwrap();
        let wal_before = fs::read(root.join("source.db-wal")).unwrap();
        let snapshot = create_history_snapshot(&path, &app_temp).unwrap();
        assert!(
            !app_temp.parent().unwrap().exists(),
            "no app work files, even while snapshot is live"
        );
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(fs::read(root.join("source.db-wal")).unwrap(), wal_before);
        source
            .execute("UPDATE t SET value='later write'", [])
            .unwrap();
        let value: String = snapshot
            .connection()
            .query_row("SELECT value FROM t", [], |r| r.get(0))
            .unwrap();
        assert_eq!(value, "latest private WAL record");
        assert!(snapshot.connection().execute("DELETE FROM t", []).is_err());
        let file: String = snapshot
            .connection()
            .query_row(
                "SELECT file FROM pragma_database_list WHERE name='main'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(file.is_empty());
        assert_eq!(
            snapshot
                .connection()
                .pragma_query_value(None, "temp_store", |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
        drop(snapshot);
        assert!(!app_temp.exists());
        drop(source);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn oversize_timeout_and_invalid_sources_fail_without_plaintext_fallback() {
        let root =
            std::env::temp_dir().join(format!("browsory_memory_reject_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("source.db");
        {
            let source = Connection::open(&path).unwrap();
            source
                .execute_batch("CREATE TABLE t(value TEXT); INSERT INTO t VALUES('preserve');")
                .unwrap();
        }
        let before = fs::read(&path).unwrap();
        assert!(create_snapshot_with_limits(&path, 1, Duration::from_secs(1)).is_err());
        assert!(create_snapshot_with_limits(&path, MAX_SNAPSHOT_BYTES, Duration::ZERO).is_err());
        assert!(SnapshotBudget::reserve(MAX_TOTAL_SNAPSHOT_BYTES + 1).is_err());
        let temp = root.join("temp");
        fs::write(root.join("invalid.db"), b"not a SQLite database").unwrap();
        assert!(create_history_snapshot(&root.join("invalid.db"), &temp).is_err());
        assert!(create_history_snapshot(&root.join("missing.db"), &temp).is_err());
        assert!(!root.join("missing.db").exists());
        assert!(!temp.exists());
        assert_eq!(fs::read(&path).unwrap(), before);
        let retry = create_history_snapshot(&path, &temp).unwrap();
        drop(retry);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cleanup_only_removes_safe_uuid_snapshot_directories() {
        let root =
            std::env::temp_dir().join(format!("browsory_cleanup_scope_{}", uuid::Uuid::new_v4()));
        let app_dir = root.join("app");
        let temp_dir = app_dir.join("temp");
        fs::create_dir_all(&temp_dir).unwrap();

        let orphan = temp_dir.join(Uuid::new_v4().to_string());
        fs::create_dir_all(orphan.join("nested")).unwrap();
        fs::write(orphan.join("nested").join("snapshot.bin"), b"snapshot").unwrap();
        let unknown_file = temp_dir.join("keep.txt");
        fs::write(&unknown_file, b"keep").unwrap();
        let unknown_dir = temp_dir.join("keep-dir");
        fs::create_dir_all(&unknown_dir).unwrap();
        fs::write(unknown_dir.join("sentinel"), b"keep").unwrap();
        let uuid_file = temp_dir.join(Uuid::new_v4().to_string());
        fs::write(&uuid_file, b"not-a-directory").unwrap();

        let outside = root.join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("sentinel"), b"outside").unwrap();
        let linked_tree = temp_dir.join(Uuid::new_v4().to_string());
        fs::create_dir_all(&linked_tree).unwrap();
        let link = linked_tree.join("outside");
        #[cfg(unix)]
        let link_result = std::os::unix::fs::symlink(&outside, &link);
        #[cfg(windows)]
        let link_result = std::os::windows::fs::symlink_dir(&outside, &link);

        if link_result.is_err() {
            // No link was created; this branch is not link-coverage evidence.
            fs::rename(&linked_tree, temp_dir.join("unavailable-link-test")).unwrap();
        }

        let (deleted, bytes) = cleanup_orphan_snapshots(&app_dir, &temp_dir);
        assert_eq!(deleted, 1);
        assert_eq!(bytes, b"snapshot".len() as u64);
        assert!(!orphan.exists());
        assert!(unknown_file.exists());
        assert!(unknown_dir.exists());
        assert!(uuid_file.exists());
        if link_result.is_ok() {
            assert!(linked_tree.exists());
        }
        assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"outside");

        if link_result.is_ok() {
            #[cfg(windows)]
            fs::remove_dir(&link).unwrap();
            #[cfg(unix)]
            fs::remove_file(&link).unwrap();
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cleanup_rejects_wrong_or_root_boundaries() {
        let root =
            std::env::temp_dir().join(format!("browsory_cleanup_reject_{}", uuid::Uuid::new_v4()));
        let app_dir = root.join("app");
        let temp_dir = app_dir.join("temp");
        fs::create_dir_all(&temp_dir).unwrap();
        let orphan = temp_dir.join(Uuid::new_v4().to_string());
        fs::create_dir_all(&orphan).unwrap();
        assert_eq!(cleanup_orphan_snapshots(&app_dir, &app_dir), (0, 0));
        assert!(orphan.exists());
        assert_eq!(cleanup_orphan_snapshots(&app_dir, &root), (0, 0));
        assert!(orphan.exists());
        assert_eq!(cleanup_orphan_snapshots(&app_dir, &temp_dir), (1, 0));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn exclusive_locked_browser_persist_and_wal_dbs_snapshot_in_memory() {
        for wal_mode in [false, true] {
            let root = std::env::temp_dir().join(format!(
                "browsory_exclusive_snapshot_{}_{}",
                wal_mode,
                uuid::Uuid::new_v4()
            ));
            fs::create_dir_all(&root).unwrap();
            let path = root.join("History");
            let temp_dir = root.join("app").join("temp");

            let holder = Connection::open(&path).unwrap();
            holder
                .query_row(
                    if wal_mode {
                        "PRAGMA journal_mode=WAL"
                    } else {
                        "PRAGMA journal_mode=PERSIST"
                    },
                    [],
                    |r| r.get::<_, String>(0),
                )
                .unwrap();
            holder
                .query_row("PRAGMA locking_mode=EXCLUSIVE", [], |r| {
                    r.get::<_, String>(0)
                })
                .unwrap();
            holder
                .execute_batch(
                    "CREATE TABLE urls(id INTEGER PRIMARY KEY, url TEXT, title TEXT);
                     CREATE TABLE visits(id INTEGER PRIMARY KEY, url INTEGER, visit_time INTEGER);
                     INSERT INTO urls VALUES(1, 'https://chrome.example', 'Chrome Live');
                     INSERT INTO visits VALUES(10, 1, 13380000000000000);",
                )
                .unwrap();
            if !wal_mode {
                // Trigger PERSIST zeroed rollback journal file just like desktop Chrome.
                holder
                    .execute(
                        "INSERT INTO urls VALUES(2, 'https://chrome.example/2', 'Chrome Second')",
                        [],
                    )
                    .unwrap();
            }

            assert!(probe_supported_history_db(&path));
            let guard = create_history_snapshot(&path, &temp_dir).unwrap();
            assert!(detect_history_schema_from_conn(guard.connection()).is_ok());
            let count: i64 = guard
                .connection()
                .query_row("SELECT COUNT(*) FROM urls", [], |r| r.get(0))
                .unwrap();
            assert_eq!(count, if wal_mode { 1 } else { 2 });
            assert!(!temp_dir.exists());

            drop(guard);
            drop(holder);
            let _ = fs::remove_dir_all(root);
        }
    }
}
