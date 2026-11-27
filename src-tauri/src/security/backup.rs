use chrono::Utc;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::envelope::{
    unwrap_master_key_with_dpapi, unwrap_master_key_with_password, unwrap_master_key_with_recovery,
    MasterKey, VaultEnvelope, MAX_ENVELOPE_BYTES,
};
use crate::database::sqlcipher::{check_encrypted_integrity, open_encrypted_database};
use crate::database::{models::BackupInfo, repository};
use crate::error::AppResult;

#[path = "../database/repository/backup_maintenance.rs"]
pub(crate) mod backup_maintenance;

const DB: &str = "archive.db";
const ENV: &str = "vault.envelope.json";
const MANIFEST: &str = "manifest.json";
pub const MAX_DATABASE_BYTES: u64 = 64 * 1024 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const SPACE_RESERVE: u64 = 16 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupManifest {
    pub format_version: u32,
    pub app_version: String,
    pub created_at: i64,
    pub db_sha256: String,
    pub envelope_sha256: String,
    pub notes: Option<String>,
}

pub fn compute_file_sha256(path: &Path) -> AppResult<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn sync_file(path: &Path) -> Result<(), String> {
    fs::OpenOptions::new()
        .write(true)
        .open(path)
        .and_then(|file| file.sync_all())
        .map_err(|e| e.to_string())
}
fn ensure_capacity(directory: &Path, required: u64) -> Result<(), String> {
    let available = fs2::available_space(directory)
        .map_err(|e| format!("Cannot inspect available disk space: {e}"))?;
    if available < required {
        return Err(format!(
            "Insufficient disk space: need {required} bytes, available {available}"
        ));
    }
    Ok(())
}
fn hash(path: &Path) -> Result<String, String> {
    compute_file_sha256(path).map_err(|e| e.to_string())
}

fn hash_checked<F>(path: &Path, check: &F) -> Result<String, String>
where
    F: Fn() -> Result<(), String>,
{
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        check()?;
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn write_orphan_backup_marker(
    backups_dir: &Path,
    file_name: &str,
    published: &Path,
    bytes: u64,
    reason: &str,
) -> Result<PathBuf, String> {
    // A published package must remain usable if its database registration
    // fails. The marker makes it discoverable by maintenance tooling and by
    // support, while restore_database_backup_file can already consume the
    // preserved package directly.
    let marker = backups_dir.join(format!(".{file_name}.orphan.json"));
    let payload = serde_json::json!({
        "format_version": 1,
        "file_name": file_name,
        "file_path": published.to_string_lossy(),
        "file_size_bytes": bytes,
        "created_at": Utc::now().timestamp_millis(),
        "record_error": reason,
    });
    let bytes = serde_json::to_vec_pretty(&payload).map_err(|error| error.to_string())?;
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&marker)
            .map_err(|error| format!("创建孤儿备份追踪标记失败: {}", error))?;
        file.write_all(&bytes)
            .map_err(|error| format!("写入孤儿备份追踪标记失败: {}", error))?;
        file.sync_all()
            .map_err(|error| format!("同步孤儿备份追踪标记失败: {}", error))?;
        drop(file);
        #[cfg(unix)]
        fs::File::open(backups_dir)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| format!("同步孤儿备份目录失败: {}", error))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&marker);
    }
    result.map(|()| marker)
}

pub struct BackupVaultContext<'a> {
    pub app_dir: &'a Path,
    pub backups_dir: &'a Path,
    pub key: &'a MasterKey,
    pub envelope: &'a VaultEnvelope,
}

/// The ZIP itself is not encrypted. User notes remain only in encrypted DB data.
/// The snapshot is reopened with the actual key before publishing the package.
pub fn execute_encrypted_backup(
    conn: &Connection,
    app_dir: &Path,
    backups_dir: &Path,
    prefix: &str,
    notes: Option<&str>,
    key: &MasterKey,
    envelope: &VaultEnvelope,
) -> Result<BackupInfo, String> {
    execute_encrypted_backup_checked(
        conn,
        BackupVaultContext {
            app_dir,
            backups_dir,
            key,
            envelope,
        },
        prefix,
        notes,
        || Ok(()),
    )
}

struct StagedBackupWork {
    file_name: String,
    published: PathBuf,
    partial: PathBuf,
    stage: PathBuf,
    staged_db: PathBuf,
}

fn prepare_staged_backup<F>(
    conn: &Connection,
    app_dir: &Path,
    backups_dir: &Path,
    envelope: &VaultEnvelope,
    prefix: &str,
    check: &F,
) -> Result<StagedBackupWork, String>
where
    F: Fn() -> Result<(), String>,
{
    check()?;
    if prefix.is_empty()
        || prefix.len() > 80
        || !prefix
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
    {
        return Err("Invalid backup file prefix".into());
    }
    envelope.validate_constraints()?;
    if conn
        .path()
        .map(Path::new)
        .map(crate::database::encrypt_migration::is_plain_sqlite_database)
        .unwrap_or(true)
    {
        return Err("拒绝对明文或内存数据库生成默认保险库备份".into());
    }
    fs::create_dir_all(backups_dir).map_err(|e| e.to_string())?;
    let pages: u64 = conn
        .pragma_query_value(None, "page_count", |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let page_size: rusqlite::types::Value = conn
        .pragma_query_value(None, "page_size", |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let page_size: u64 = match page_size {
        rusqlite::types::Value::Integer(size) if size > 0 => size as u64,
        rusqlite::types::Value::Text(size) => {
            size.parse().map_err(|_| "无效的 SQLCipher 页面大小")?
        }
        _ => return Err("无效的 SQLCipher 页面大小".into()),
    };
    let required = pages
        .saturating_mul(page_size)
        .saturating_mul(2)
        .saturating_add(SPACE_RESERVE);
    ensure_capacity(app_dir, required)?;
    ensure_capacity(backups_dir, required)?;
    let id = Uuid::new_v4();
    let file_name = format!(
        "{prefix}_{}_{id}.browsory-backup",
        Utc::now().format("%Y%m%d_%H%M%S")
    );
    let published = backups_dir.join(&file_name);
    let partial = backups_dir.join(format!(".{file_name}.partial"));
    let stage = app_dir.join("backup-work").join(id.to_string());
    fs::create_dir_all(&stage).map_err(|e| e.to_string())?;
    let staged_db = stage.join(DB);
    let snapshot_res = (|| {
        check()?;
        // Parameter binding also supports apostrophes and non-ASCII data paths.
        conn.execute(
            "VACUUM main INTO ?1",
            [staged_db.to_string_lossy().as_ref()],
        )
        .map_err(|e| e.to_string())?;
        check()?;
        Ok(())
    })();
    if let Err(err) = snapshot_res {
        let _ = fs::remove_dir_all(&stage);
        return Err(err);
    }
    Ok(StagedBackupWork {
        file_name,
        published,
        partial,
        stage,
        staged_db,
    })
}

fn package_and_publish_backup<F>(
    work: &StagedBackupWork,
    key: &MasterKey,
    envelope: &VaultEnvelope,
    backups_dir: &Path,
    check: &F,
    was_published: &mut bool,
    published_bytes: &mut u64,
) -> Result<u64, String>
where
    F: Fn() -> Result<(), String>,
{
    envelope.save_atomic(&work.stage)?;
    check()?;
    {
        let snapshot = open_encrypted_database(&work.staged_db, key.as_bytes(), false)
            .map_err(|e| e.to_string())?;
        if !check_encrypted_integrity(&snapshot).map_err(|e| e.to_string())? {
            return Err("Encrypted backup integrity check failed".into());
        }
        let busy: i64 = snapshot
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if busy != 0 {
            return Err("Backup WAL checkpoint did not complete".into());
        }
    }
    check()?;
    sync_file(&work.staged_db)?;
    let manifest = BackupManifest {
        format_version: 1,
        app_version: env!("CARGO_PKG_VERSION").into(),
        created_at: Utc::now().timestamp_millis(),
        db_sha256: hash_checked(&work.staged_db, check)?,
        envelope_sha256: hash_checked(&work.stage.join(ENV), check)?,
        notes: None,
    };
    fs::write(
        work.stage.join(MANIFEST),
        serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&work.partial)
        .map_err(|e| e.to_string())?;
    let mut zip = ZipWriter::new(file);
    // SQLCipher DB pages are AES-encrypted high-entropy ciphertext; storing without Deflate
    // eliminates pointless CPU work and avoids holding locks during compression.
    let stored_options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o600);
    let deflated_options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o600);
    for name in [DB, ENV, MANIFEST] {
        check()?;
        let options = if name == DB {
            stored_options
        } else {
            deflated_options
        };
        zip.start_file(name, options).map_err(|e| e.to_string())?;
        let mut input = File::open(work.stage.join(name)).map_err(|e| e.to_string())?;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            check()?;
            let read = input.read(&mut buffer).map_err(|e| e.to_string())?;
            if read == 0 {
                break;
            }
            zip.write_all(&buffer[..read]).map_err(|e| e.to_string())?;
        }
    }
    check()?;
    zip.finish()
        .map_err(|e| e.to_string())?
        .sync_all()
        .map_err(|e| e.to_string())?;
    check()?;
    fs::rename(&work.partial, &work.published).map_err(|e| e.to_string())?;
    *was_published = true;
    #[cfg(unix)]
    if let Err(error) = fs::File::open(backups_dir).and_then(|directory| directory.sync_all()) {
        // The package is already published and remains usable. Directory
        // fsync failure is an observability warning, not a false failure
        // that could cause the caller to retry and create duplicates.
        tracing::warn!(
            backup = %work.published.display(),
            error = %error,
            "Backup package published but directory sync failed"
        );
    }
    #[cfg(not(unix))]
    let _ = backups_dir;
    let bytes = fs::metadata(&work.published)
        .map_err(|e| e.to_string())?
        .len();
    *published_bytes = bytes;
    Ok(bytes)
}

fn finalize_or_cleanup_backup<R>(
    work: &StagedBackupWork,
    backups_dir: &Path,
    was_published: bool,
    published_bytes: u64,
    result: Result<R, String>,
) -> Result<R, String> {
    // Known UUID workspace only; neither the backup nor its directory is removed.
    let _ = fs::remove_dir_all(&work.stage);
    if result.is_err() {
        let _ = fs::remove_file(&work.partial);
    }
    match result {
        Ok(backup) => Ok(backup),
        Err(error) if was_published => {
            let tracking = write_orphan_backup_marker(
                backups_dir,
                &work.file_name,
                &work.published,
                published_bytes,
                &error,
            );
            match tracking {
                Ok(marker) => Err(format!(
                    "备份文件已发布但登记失败，文件已保留：{}；追踪标记：{}；原始错误：{}",
                    work.published.display(),
                    marker.display(),
                    error
                )),
                Err(marker_error) => {
                    tracing::error!(
                        backup = %work.published.display(),
                        marker_error = %marker_error,
                        "Backup registration failed; published package was preserved without marker"
                    );
                    Err(format!(
                        "备份文件已发布但登记失败，文件已保留：{}；追踪标记写入失败：{}；原始错误：{}",
                        work.published.display(),
                        marker_error,
                        error
                    ))
                }
            }
        }
        Err(error) => Err(error),
    }
}

/// Creates and publishes an encrypted backup while acquiring the vault write lock only for
/// staging (`VACUUM INTO`) and final DB metadata registration, leaving integrity verification,
/// SHA-256 hashing, ZIP packaging, and atomic rename lock-free.
pub fn execute_encrypted_backup_with_operation(
    operation: &crate::security::session::VaultOperation,
    context: BackupVaultContext<'_>,
    prefix: &str,
    notes: Option<&str>,
) -> Result<BackupInfo, String> {
    let BackupVaultContext {
        app_dir,
        backups_dir,
        key,
        envelope,
    } = context;
    let check = || operation.check().map_err(|e| e.to_string());
    let work = operation
        .with_conn(|conn| {
            prepare_staged_backup(conn, app_dir, backups_dir, envelope, prefix, &check)
                .map_err(crate::error::AppError::System)
        })
        .map_err(|e| e.to_string())?;
    let mut was_published = false;
    let mut published_bytes = 0u64;
    let result = (|| {
        let bytes = package_and_publish_backup(
            &work,
            key,
            envelope,
            backups_dir,
            &check,
            &mut was_published,
            &mut published_bytes,
        )?;
        let published_str = work.published.to_string_lossy().into_owned();
        operation
            .with_conn(|conn| {
                repository::record_backup(conn, &work.file_name, &published_str, bytes, notes)
            })
            .map_err(|e| e.to_string())
    })();
    finalize_or_cleanup_backup(&work, backups_dir, was_published, published_bytes, result)
}

/// Creates and publishes an encrypted backup while polling a caller-owned
/// cancellation probe at every bounded I/O boundary. Once the package has
/// been renamed into place, registration is completed even if the session is
/// revoked so a successful operation cannot be reported as a false failure.
pub fn execute_encrypted_backup_checked<F>(
    conn: &Connection,
    context: BackupVaultContext<'_>,
    prefix: &str,
    notes: Option<&str>,
    check: F,
) -> Result<BackupInfo, String>
where
    F: Fn() -> Result<(), String>,
{
    let BackupVaultContext {
        app_dir,
        backups_dir,
        key,
        envelope,
    } = context;
    let work = prepare_staged_backup(conn, app_dir, backups_dir, envelope, prefix, &check)?;
    let mut was_published = false;
    let mut published_bytes = 0u64;
    let result = (|| {
        let bytes = package_and_publish_backup(
            &work,
            key,
            envelope,
            backups_dir,
            &check,
            &mut was_published,
            &mut published_bytes,
        )?;
        repository::record_backup(
            conn,
            &work.file_name,
            &work.published.to_string_lossy(),
            bytes,
            notes,
        )
        .map_err(|e| e.to_string())
    })();
    finalize_or_cleanup_backup(&work, backups_dir, was_published, published_bytes, result)
}

// Our writer emits no archive comment or self-extracting prefix. Bound metadata
// before ZipArchive allocates from untrusted counts (PKWARE APPNOTE 4.3.14-16).
fn preflight_zip(file: &mut File) -> Result<(), String> {
    fn u16_at(bytes: &[u8], at: usize) -> u16 {
        u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap())
    }
    fn u32_at(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
    }
    fn u64_at(bytes: &[u8], at: usize) -> u64 {
        u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
    }
    let length = file.metadata().map_err(|e| e.to_string())?.len();
    if !(22..=MAX_DATABASE_BYTES * 2 + 2 * 1024 * 1024).contains(&length) {
        return Err("备份包总大小超出限制".into());
    }
    file.seek(SeekFrom::End(-22)).map_err(|e| e.to_string())?;
    let mut end = [0u8; 22];
    file.read_exact(&mut end).map_err(|e| e.to_string())?;
    if &end[..4] != b"PK\x05\x06"
        || u16_at(&end, 20) != 0
        || u16_at(&end, 4) != 0
        || u16_at(&end, 6) != 0
    {
        return Err("备份包结束记录无效，不支持分卷或 ZIP 注释".into());
    }
    let mut count = u16_at(&end, 10) as u64;
    let mut per_disk = u16_at(&end, 8) as u64;
    let mut size = u32_at(&end, 12) as u64;
    let mut offset = u32_at(&end, 16) as u64;
    let mut boundary = length - 22;
    if length >= 42 {
        file.seek(SeekFrom::End(-42)).map_err(|e| e.to_string())?;
        let mut locator = [0u8; 20];
        file.read_exact(&mut locator).map_err(|e| e.to_string())?;
        if &locator[..4] == b"PK\x06\x07" {
            let record_offset = u64_at(&locator, 8);
            if u32_at(&locator, 4) != 0
                || u32_at(&locator, 16) != 1
                || record_offset.checked_add(56) != Some(length - 42)
            {
                return Err("ZIP64 定位记录无效".into());
            }
            file.seek(SeekFrom::Start(record_offset))
                .map_err(|e| e.to_string())?;
            let mut record = [0u8; 56];
            file.read_exact(&mut record).map_err(|e| e.to_string())?;
            if &record[..4] != b"PK\x06\x06"
                || u64_at(&record, 4) != 44
                || u32_at(&record, 16) != 0
                || u32_at(&record, 20) != 0
            {
                return Err("ZIP64 目录记录无效".into());
            }
            count = u64_at(&record, 32);
            per_disk = u64_at(&record, 24);
            size = u64_at(&record, 40);
            offset = u64_at(&record, 48);
            boundary = record_offset;
            if ![3, u16::MAX].contains(&u16_at(&end, 8))
                || ![3, u16::MAX].contains(&u16_at(&end, 10))
            {
                return Err("ZIP 与 ZIP64 文件数量不一致".into());
            }
        }
    }
    if count != 3
        || per_disk != 3
        || !(3 * 46..=256 * 1024).contains(&size)
        || offset.checked_add(size) != Some(boundary)
    {
        return Err("备份目录数量、大小或偏移超出限制".into());
    }
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    Ok(())
}

/// Validates an archive's ZIP structure and bounded manifest without decrypting
/// or extracting payloads, returning the recorded manifest timestamp.
pub(crate) fn verify_backup_package_created_at(backup: &Path) -> Result<i64, String> {
    let mut file = File::open(backup).map_err(|e| e.to_string())?;
    preflight_zip(&mut file)?;
    let mut zip = ZipArchive::with_config(
        zip::read::Config {
            archive_offset: zip::read::ArchiveOffset::Known(0),
        },
        file,
    )
    .map_err(|e| format!("Invalid encrypted backup package: {e}"))?;
    if zip.len() != 3 {
        return Err("Backup must contain exactly three known root files".into());
    }
    let mut names = HashSet::new();
    for index in 0..zip.len() {
        let entry = zip.by_index(index).map_err(|e| e.to_string())?;
        let name = entry
            .name()
            .map_err(|error| format!("Invalid backup entry name: {error}"))?;
        let limit = match name.as_ref() {
            DB => MAX_DATABASE_BYTES,
            ENV => MAX_ENVELOPE_BYTES,
            MANIFEST => MAX_MANIFEST_BYTES,
            _ => return Err("Unexpected backup path or component".into()),
        };
        if entry.is_dir()
            || entry
                .unix_mode()
                .map(|mode| mode & 0o170000 == 0o120000)
                .unwrap_or(false)
            || !names.insert(name.to_string())
            || entry.size() == 0
            || entry.size() > limit
        {
            return Err("Invalid, duplicate, symlink or oversized backup component".into());
        }
    }
    let manifest_entry = zip.by_name(MANIFEST).map_err(|e| e.to_string())?;
    let mut manifest_bytes = Vec::new();
    manifest_entry
        .take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut manifest_bytes)
        .map_err(|e| e.to_string())?;
    if manifest_bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err("Backup manifest exceeded size limit".into());
    }
    let manifest: BackupManifest =
        serde_json::from_slice(&manifest_bytes).map_err(|e| e.to_string())?;
    if manifest.format_version != 1 {
        return Err("Unsupported backup format version".into());
    }
    Ok(manifest.created_at)
}

fn extract_bundle(backup: &Path, directory: &Path) -> Result<(), String> {
    let mut file = File::open(backup).map_err(|e| e.to_string())?;
    preflight_zip(&mut file)?;
    let mut zip = ZipArchive::with_config(
        zip::read::Config {
            archive_offset: zip::read::ArchiveOffset::Known(0),
        },
        file,
    )
    .map_err(|e| format!("Invalid encrypted backup package: {e}"))?;
    if zip.len() != 3 {
        return Err("Backup must contain exactly three known root files".into());
    }
    let mut names = HashSet::new();
    let mut database_bytes = 0;
    // Validate the entire directory before extracting any payload.
    for index in 0..zip.len() {
        let entry = zip.by_index(index).map_err(|e| e.to_string())?;
        let name = entry
            .name()
            .map_err(|error| format!("Invalid backup entry name: {error}"))?;
        let limit = match name.as_ref() {
            DB => MAX_DATABASE_BYTES,
            ENV => MAX_ENVELOPE_BYTES,
            MANIFEST => MAX_MANIFEST_BYTES,
            _ => return Err("Unexpected backup path or component".into()),
        };
        if entry.is_dir()
            || entry
                .unix_mode()
                .map(|mode| mode & 0o170000 == 0o120000)
                .unwrap_or(false)
            || !names.insert(name.to_string())
            || entry.size() == 0
            || entry.size() > limit
        {
            return Err("Invalid, duplicate, symlink or oversized backup component".into());
        }
        if name == DB {
            database_bytes = entry.size();
        }
    }
    let old_bytes = fs::metadata(directory.join(DB))
        .map(|file| file.len())
        .unwrap_or(0);
    ensure_capacity(
        directory,
        database_bytes
            .saturating_mul(2)
            .saturating_add(old_bytes.saturating_mul(2))
            .saturating_add(SPACE_RESERVE),
    )?;
    let incoming = directory.join(super::restore::WORKSPACE).join("incoming");
    fs::create_dir_all(&incoming).map_err(|e| e.to_string())?;
    for index in 0..zip.len() {
        let entry = zip.by_index(index).map_err(|e| e.to_string())?;
        let expected = entry.size();
        let path = incoming.join(
            entry
                .name()
                .map_err(|error| format!("Invalid backup entry name: {error}"))?
                .as_ref(),
        );
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| e.to_string())?;
        let copied =
            std::io::copy(&mut entry.take(expected + 1), &mut output).map_err(|e| e.to_string())?;
        if copied != expected {
            return Err("Backup component exceeded its declared size or was truncated".into());
        }
        output.sync_all().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Validates before requesting lifecycle ownership. File replacement is handled
/// by the durable restore journal after the callback has drained old work.
pub fn restore_encrypted_backup_flow<F>(
    backup_path: &Path,
    app_dir: &Path,
    password: Option<&str>,
    recovery_key: Option<&str>,
    current_master_key: Option<&MasterKey>,
    current_envelope: Option<&VaultEnvelope>,
    before_replace: F,
) -> Result<(MasterKey, VaultEnvelope), String>
where
    F: FnOnce() -> Result<(), String>,
{
    if app_dir.join("restore.state.json").exists() {
        return Err("A previous restore must be recovered before starting another".into());
    }
    let workspace = app_dir.join(super::restore::WORKSPACE);
    // create_dir is exclusive: another restore may already be validating.
    fs::create_dir(&workspace).map_err(|e| format!("Cannot reserve restore workspace: {e}"))?;
    let mut replacing = false;
    let result = (|| {
        extract_bundle(backup_path, app_dir)?;
        let stage = workspace.join("incoming");
        let bytes = fs::read(stage.join(MANIFEST)).map_err(|e| e.to_string())?;
        let manifest: BackupManifest = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if manifest.format_version != 1 {
            return Err("Unsupported backup format version".into());
        }
        if hash(&stage.join(DB))? != manifest.db_sha256
            || hash(&stage.join(ENV))? != manifest.envelope_sha256
        {
            return Err("Backup database or envelope hash mismatch".into());
        }
        let envelope = VaultEnvelope::load_from_dir(&stage)?.ok_or("Missing backup envelope")?;
        let master_key = if let Some(password) = password {
            unwrap_master_key_with_password(&envelope, password)?
        } else if let Some(recovery) = recovery_key {
            unwrap_master_key_with_recovery(&envelope, recovery)?
        } else if current_envelope == Some(&envelope) {
            current_master_key
                .cloned()
                .ok_or("Current vault key is unavailable")?
        } else if envelope.has_dpapi_wrap() {
            unwrap_master_key_with_dpapi(&envelope).map_err(|e| {
                format!(
                    "无法使用当前系统的本机密钥自动解封备份（{}），请提供备份口令或恢复密钥",
                    e
                )
            })?
        } else {
            return Err(
                "Backup credentials differ: provide its original passphrase or recovery key".into(),
            );
        };
        {
            let mut conn = open_encrypted_database(&stage.join(DB), master_key.as_bytes(), false)
                .map_err(|e| e.to_string())?;
            if !check_encrypted_integrity(&conn).map_err(|e| e.to_string())? {
                return Err("Backup database integrity check failed".into());
            }
            let version: i32 = conn
                .query_row(
                    "SELECT COALESCE(MAX(version),0) FROM schema_migrations",
                    [],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            if version > crate::database::migrations::latest_schema_version() {
                return Err("Backup schema is newer than this application".into());
            }
            crate::database::migrations::run_migrations(&mut conn).map_err(|e| e.to_string())?;
            repository::query_history(&conn, &crate::database::HistoryFilter::default())
                .map_err(|e| e.to_string())?;
            let _: i64 = conn
                .query_row(
                    "SELECT count(*) FROM urls_fts WHERE urls_fts MATCH 'browsoryrestorecheck'",
                    [],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            let foreign_keys: i64 = conn
                .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                    r.get(0)
                })
                .map_err(|e| e.to_string())?;
            if foreign_keys != 0 || !check_encrypted_integrity(&conn).map_err(|e| e.to_string())? {
                return Err("Restored schema or foreign keys are invalid".into());
            }
            let busy: i64 = conn
                .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))
                .map_err(|e| e.to_string())?;
            if busy != 0 {
                return Err("Backup WAL checkpoint is busy".into());
            }
        }
        before_replace()?;
        replacing = true;
        let old_key =
            current_master_key.ok_or("Current vault must be unlocked before replacement")?;
        let old_envelope = current_envelope.ok_or("Current vault metadata is unavailable")?;
        super::restore::install(app_dir, old_key, old_envelope, &master_key)?;
        Ok((master_key, envelope))
    })();
    if result.is_err() && !replacing {
        // No live-file mutation has occurred. This exact reserved workspace is
        // the only directory removed; root backups and original files remain.
        let _ = fs::remove_dir_all(&workspace);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::migrations::run_migrations;
    use crate::security::envelope::create_vault_envelope;
    use std::path::PathBuf;

    #[test]
    fn zip_metadata_limits_apply_before_archive_allocation_including_zip64() {
        use std::io::{Cursor, Write};
        let root = std::env::temp_dir().join(format!("browsory_zip_limits_{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("metadata.zip");
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for name in [DB, ENV, MANIFEST] {
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(b"x").unwrap();
        }
        let valid = zip.finish().unwrap().into_inner();
        let end_at = valid.len() - 22;
        let dir_size =
            u32::from_le_bytes(valid[end_at + 12..end_at + 16].try_into().unwrap()) as u64;
        let dir_offset =
            u32::from_le_bytes(valid[end_at + 16..end_at + 20].try_into().unwrap()) as u64;
        fs::write(&path, &valid).unwrap();
        assert!(preflight_zip(&mut File::open(&path).unwrap()).is_ok());
        let mut wrong_count = valid.clone();
        wrong_count[end_at + 8..end_at + 12].copy_from_slice(&[255; 4]);
        fs::write(&path, wrong_count).unwrap();
        assert!(preflight_zip(&mut File::open(&path).unwrap()).is_err());
        for (count, size, accepted) in [
            (3u64, dir_size, true),
            (u64::MAX, dir_size, false),
            (3, 512 * 1024, false),
        ] {
            let mut bytes = valid[..end_at].to_vec();
            let mut record = [0u8; 56];
            record[..4].copy_from_slice(b"PK\x06\x06");
            record[4..12].copy_from_slice(&44u64.to_le_bytes());
            record[24..32].copy_from_slice(&count.to_le_bytes());
            record[32..40].copy_from_slice(&count.to_le_bytes());
            record[40..48].copy_from_slice(&size.to_le_bytes());
            record[48..56].copy_from_slice(&dir_offset.to_le_bytes());
            bytes.extend_from_slice(&record);
            let mut locator = [0u8; 20];
            locator[..4].copy_from_slice(b"PK\x06\x07");
            locator[8..16].copy_from_slice(&(end_at as u64).to_le_bytes());
            locator[16..20].copy_from_slice(&1u32.to_le_bytes());
            bytes.extend_from_slice(&locator);
            bytes.extend_from_slice(&valid[end_at..]);
            fs::write(&path, bytes).unwrap();
            assert_eq!(
                preflight_zip(&mut File::open(&path).unwrap()).is_ok(),
                accepted
            );
            if accepted {
                assert_eq!(
                    ZipArchive::new(File::open(&path).unwrap()).unwrap().len(),
                    3
                );
            }
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn duplicate_central_directory_names_are_rejected_without_extracting() {
        use std::io::{Cursor, Write};
        let root = std::env::temp_dir().join(format!("browsory_zip_duplicate_{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for name in [DB, "archive.dx", MANIFEST] {
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(b"x").unwrap();
        }
        let mut bytes = zip.finish().unwrap().into_inner();
        for index in 0..bytes.len() - 10 {
            if &bytes[index..index + 10] == b"archive.dx" {
                bytes[index + 9] = b'b';
            }
        }
        let package = root.join("duplicate.zip");
        fs::write(&package, bytes).unwrap();
        assert!(extract_bundle(&package, &root).is_err());
        assert!(!root.join(super::super::restore::WORKSPACE).exists());
        fs::remove_dir_all(root).unwrap();
    }

    fn fixture() -> (PathBuf, crate::security::vault::VaultManager) {
        let root = std::env::temp_dir().join(format!("browsory_backup_'_{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let vault = crate::security::vault::VaultManager::unlocked_fixture(root.clone());
        (root, vault)
    }

    fn backup(root: &Path, vault: &crate::security::vault::VaultManager) -> PathBuf {
        vault
            .with_conn(|conn| {
                Ok(PathBuf::from(
                    execute_encrypted_backup(
                        conn,
                        root,
                        &root.join("backups"),
                        "test",
                        Some("private note must stay encrypted"),
                        &vault.master_key().unwrap(),
                        &vault.envelope().unwrap(),
                    )
                    .unwrap()
                    .file_path,
                ))
            })
            .unwrap()
    }

    #[test]
    fn wrong_credentials_and_changed_envelope_leave_live_session_untouched() {
        use std::cell::Cell;
        let (root, vault) = fixture();
        let package = backup(&root, &vault);
        let before_db = hash(&root.join(DB)).unwrap();
        let before_env = fs::read(root.join(ENV)).unwrap();
        let key = vault.master_key().unwrap();
        let mut changed_envelope = vault.envelope().unwrap();
        changed_envelope.recovery_key_confirmed = !changed_envelope.recovery_key_confirmed;
        for (password, envelope) in [
            (Some("wrong password"), vault.envelope().unwrap()),
            (None, changed_envelope),
        ] {
            let called = Cell::new(false);
            assert!(restore_encrypted_backup_flow(
                &package,
                &root,
                password,
                None,
                Some(&key),
                Some(&envelope),
                || {
                    called.set(true);
                    Ok(())
                }
            )
            .is_err());
            assert!(
                !called.get(),
                "validation must precede live-session closure"
            );
            assert_eq!(hash(&root.join(DB)).unwrap(), before_db);
            assert_eq!(fs::read(root.join(ENV)).unwrap(), before_env);
            assert!(vault.connection().unwrap().lock().is_ok());
            assert!(!root.join(super::super::restore::WORKSPACE).exists());
        }
        vault.lock().unwrap();
        drop(vault);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn snapshot_includes_live_wal_notes_stay_private_and_filenames_do_not_collide() {
        let (root, vault) = fixture();
        vault.with_conn(|conn| {
            conn.query_row("PRAGMA wal_autocheckpoint=0", [], |r| r.get::<_, i64>(0))?;
            conn.execute_batch("CREATE TABLE backup_sentinel(value TEXT); INSERT INTO backup_sentinel VALUES('latest WAL');")?;
            Ok(())
        }).unwrap();
        let first = backup(&root, &vault);
        let second = backup(&root, &vault);
        assert_ne!(first, second);
        let mut package = ZipArchive::new(File::open(&first).unwrap()).unwrap();
        let mut metadata = String::new();
        package
            .by_name(MANIFEST)
            .unwrap()
            .read_to_string(&mut metadata)
            .unwrap();
        assert!(!metadata.contains("private note"));
        assert!(serde_json::from_str::<BackupManifest>(&metadata)
            .unwrap()
            .notes
            .is_none());
        let target = root.join("extracted");
        fs::create_dir(&target).unwrap();
        extract_bundle(&first, &target).unwrap();
        let conn = open_encrypted_database(
            &target
                .join(super::super::restore::WORKSPACE)
                .join("incoming")
                .join(DB),
            vault.master_key().unwrap().as_bytes(),
            false,
        )
        .unwrap();
        assert_eq!(
            conn.query_row("SELECT value FROM backup_sentinel", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "latest WAL"
        );
        drop(conn);
        drop(package);
        vault.lock().unwrap();
        drop(vault);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_backup_registration_preserves_package_and_writes_orphan_marker() {
        let (root, vault) = fixture();
        vault
            .with_conn(|conn| {
                conn.execute_batch(
                    "CREATE TRIGGER test_backup_record_failure
                     BEFORE INSERT ON backups
                     BEGIN SELECT RAISE(ABORT, 'injected backup registration failure'); END;",
                )?;
                Ok(())
            })
            .unwrap();

        let backups_dir = root.join("backups");
        let result = vault.with_conn(|conn| {
            execute_encrypted_backup(
                conn,
                &root,
                &backups_dir,
                "registration_failure",
                Some("must not be copied into the marker"),
                &vault.master_key().unwrap(),
                &vault.envelope().unwrap(),
            )
            .map(|_| ())
            .map_err(crate::error::AppError::System)
        });
        let error = result
            .expect_err("injected backup registration must fail")
            .to_string();
        assert!(error.contains("已保留"), "{error}");
        assert!(error.contains("追踪标记"), "{error}");

        let packages: Vec<PathBuf> = fs::read_dir(&backups_dir)
            .unwrap()
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().is_some_and(|ext| ext == "browsory-backup"))
            .collect();
        assert_eq!(packages.len(), 1);
        let package = &packages[0];
        let file_name = package.file_name().unwrap().to_string_lossy().to_string();
        let marker = backups_dir.join(format!(".{file_name}.orphan.json"));
        assert!(marker.is_file());
        let marker_json: serde_json::Value =
            serde_json::from_slice(&fs::read(&marker).unwrap()).unwrap();
        assert_eq!(marker_json["file_name"], file_name);
        assert_eq!(
            marker_json["file_path"],
            package.to_string_lossy().to_string()
        );
        assert!(!marker_json["record_error"]
            .as_str()
            .unwrap()
            .contains("must not be copied into the marker"));
        assert_eq!(
            ZipArchive::new(File::open(package).unwrap()).unwrap().len(),
            3
        );

        vault.lock().unwrap();
        drop(vault);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_packages_are_rejected_before_payload_extraction_or_replacement() {
        use std::io::Write;
        let (root, vault) = fixture();
        let before = fs::read(root.join(ENV)).unwrap();
        for (name, envelope_bytes, version, bad_hash) in [
            ("../escape", 1, 1, false),
            (DB, MAX_ENVELOPE_BYTES as usize + 1, 1, false),
            (DB, 1, 999, false),
            (DB, 1, 1, true),
        ] {
            let package = root.join("invalid.browsory-backup");
            let mut zip = ZipWriter::new(File::create(&package).unwrap());
            let env = vec![b'x'; envelope_bytes];
            let manifest = BackupManifest {
                format_version: version,
                app_version: "test".into(),
                created_at: 0,
                db_sha256: if bad_hash {
                    "0".repeat(64)
                } else {
                    format!("{:x}", Sha256::digest(b"invalid db"))
                },
                envelope_sha256: format!("{:x}", Sha256::digest(&env)),
                notes: None,
            };
            for (entry, bytes) in [
                (name, b"invalid db".to_vec()),
                (ENV, env),
                (MANIFEST, serde_json::to_vec(&manifest).unwrap()),
            ] {
                zip.start_file(entry, SimpleFileOptions::default()).unwrap();
                zip.write_all(&bytes).unwrap();
            }
            zip.finish().unwrap();
            assert!(restore_encrypted_backup_flow(
                &package,
                &root,
                None,
                None,
                vault.master_key().as_ref(),
                vault.envelope().as_ref(),
                || panic!("must never reach replacement")
            )
            .is_err());
            assert_eq!(fs::read(root.join(ENV)).unwrap(), before);
            assert!(vault.connection().unwrap().lock().is_ok());
            assert!(!root.join("escape").exists());
            assert!(!root.join(super::super::restore::WORKSPACE).exists());
        }
        vault.lock().unwrap();
        drop(vault);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn test_encrypted_backup_and_restore_lifecycle() {
        let root = std::env::temp_dir().join(format!("browsory_backup_test_{}", Uuid::new_v4()));
        let app_dir = root.join("app");
        let temp_dir = root.join("temp");
        let backups_dir = root.join("backups");
        fs::create_dir_all(&app_dir).unwrap();
        fs::create_dir_all(&temp_dir).unwrap();
        fs::create_dir_all(&backups_dir).unwrap();

        let password = "TestPassphrase!2026";
        let (envelope, _recovery_str, master_key) =
            create_vault_envelope(password, None, Utc::now().timestamp_millis()).unwrap();
        envelope.save_atomic(&app_dir).unwrap();

        let db_path = app_dir.join("archive.db");
        let mut conn = open_encrypted_database(&db_path, master_key.as_bytes(), true).unwrap();
        run_migrations(&mut conn).unwrap();

        // Insert test data
        conn.execute(
            "INSERT INTO urls (id, url, title, domain) VALUES (1, 'https://vault.browsory.test', 'Vault Test', 'browsory.test');",
            [],
        ).unwrap();

        // 1. Create encrypted backup
        let backup_info = execute_encrypted_backup(
            &conn,
            &app_dir,
            &backups_dir,
            "archive_backup",
            Some("测试备份"),
            &master_key,
            &envelope,
        )
        .unwrap();

        let backup_file = PathBuf::from(&backup_info.file_path);
        assert!(backup_file.exists());
        assert!(backup_file.to_string_lossy().ends_with(".browsory-backup"));

        // 2. Modify active database to create discrepancy and close connection
        conn.execute("UPDATE urls SET title = 'Modified Title' WHERE id = 1;", [])
            .unwrap();
        drop(conn);

        // 3. Restore from backup
        let (restored_key, _restored_env) = restore_encrypted_backup_flow(
            &backup_file,
            &app_dir,
            Some(password),
            None,
            Some(&master_key),
            Some(&envelope),
            || Ok(()),
        )
        .unwrap();

        // 4. Verify restored state
        let restored_conn =
            open_encrypted_database(&db_path, restored_key.as_bytes(), false).unwrap();
        let title: String = restored_conn
            .query_row("SELECT title FROM urls WHERE id = 1;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            title, "Vault Test",
            "Restored title must match original backup state"
        );

        let _ = fs::remove_dir_all(&root);
    }
}
