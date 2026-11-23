//! Read-only access to legacy archive attachments already stored in SQLCipher.
//! The original archive directory is never used as an authorization path.

use rusqlite::Connection;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::Path;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

pub const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_PREVIEW_BYTES: u64 = 8 * 1024 * 1024;
const MAX_FILES: u64 = 128;
const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct ArchiveFileInfo {
    pub relative_name: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArchiveFilePreview {
    pub relative_name: String,
    pub mime_type: String,
    pub bytes: Vec<u8>,
    pub sha256: String,
}

fn invalid(message: &str) -> AppError {
    AppError::Security(message.into())
}

fn validate_page_uuid(page_uuid: &str) -> AppResult<()> {
    if Uuid::parse_str(page_uuid)
        .map(|uuid| uuid.to_string() == page_uuid)
        .unwrap_or(false)
    {
        Ok(())
    } else {
        Err(invalid("归档标识格式无效"))
    }
}

fn validate_relative_name(name: &str) -> AppResult<()> {
    if name.is_empty() || name.len() > 512 || name.contains('\0') {
        return Err(invalid("归档附件名称无效"));
    }
    let normalized = name.replace('\\', "/");
    if normalized.starts_with('/')
        || normalized
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(invalid("归档附件路径包含非法组件"));
    }
    Ok(())
}

fn extension_kind(name: &str) -> (&'static str, &'static str) {
    let extension = name
        .rsplit('/')
        .next()
        .unwrap_or(name)
        .rsplit('.')
        .next()
        .unwrap_or("");
    match extension.to_ascii_lowercase().as_str() {
        "png" => ("image", "image/png"),
        "jpg" | "jpeg" => ("image", "image/jpeg"),
        "webp" => ("image", "image/webp"),
        "gif" => ("image", "image/gif"),
        "bmp" => ("image", "image/bmp"),
        "txt" | "log" | "csv" | "json" | "xml" | "md" => ("text", "text/plain"),
        "html" | "htm" | "svg" => ("document", "application/octet-stream"),
        _ => ("file", "application/octet-stream"),
    }
}

fn image_magic_matches(name: &str, bytes: &[u8]) -> bool {
    let extension = name
        .rsplit('/')
        .next()
        .unwrap_or(name)
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "jpg" | "jpeg" => bytes.starts_with(b"\xff\xd8\xff"),
        "gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "bmp" => bytes.starts_with(b"BM"),
        "webp" => bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP",
        _ => false,
    }
}

fn checked_size(size: i64, limit: u64) -> AppResult<u64> {
    let size = u64::try_from(size).map_err(|_| invalid("归档附件大小无效"))?;
    if size > limit {
        return Err(invalid("归档附件超出读取限制"));
    }
    Ok(size)
}

fn checked_hash(value: String) -> AppResult<String> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid("归档附件哈希格式无效"));
    }
    Ok(value)
}

fn read_content(
    conn: &Connection,
    page_uuid: &str,
    relative_name: &str,
    limit: u64,
) -> AppResult<(Vec<u8>, String)> {
    validate_page_uuid(page_uuid)?;
    validate_relative_name(relative_name)?;
    let (declared_size, declared_hash): (i64, String) = conn.query_row(
        "SELECT size_bytes, sha256 FROM legacy_archive_files WHERE page_uuid=?1 AND relative_name=?2",
        [page_uuid, relative_name],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let declared_size = checked_size(declared_size, limit)?;
    let declared_hash = checked_hash(declared_hash)?;
    // Read and validate the length before asking SQLite to materialize the BLOB.
    let content: Vec<u8> = conn.query_row(
        "SELECT content FROM legacy_archive_files WHERE page_uuid=?1 AND relative_name=?2 AND length(content)=?3",
        rusqlite::params![page_uuid, relative_name, declared_size as i64],
        |row| row.get(0),
    )?;
    if content.len() as u64 != declared_size
        || format!("{:x}", Sha256::digest(&content)) != declared_hash
    {
        return Err(invalid("归档附件长度或哈希校验失败"));
    }
    Ok((content, declared_hash))
}

pub fn list(conn: &Connection, page_uuid: &str) -> AppResult<Vec<ArchiveFileInfo>> {
    validate_page_uuid(page_uuid)?;
    let mut stmt = conn.prepare(
        "SELECT relative_name, size_bytes, sha256 FROM legacy_archive_files WHERE page_uuid=?1 ORDER BY relative_name",
    )?;
    let rows = stmt.query_map([page_uuid], |row| {
        let name: String = row.get(0)?;
        let size: i64 = row.get(1)?;
        let hash: String = row.get(2)?;
        Ok((name, size, hash))
    })?;
    let mut result = Vec::new();
    let mut total = 0u64;
    for row in rows {
        let (name, size, hash) = row?;
        validate_relative_name(&name)?;
        let size = checked_size(size, MAX_FILE_BYTES)?;
        let hash = checked_hash(hash)?;
        total = total
            .checked_add(size)
            .ok_or_else(|| invalid("归档附件总大小溢出"))?;
        if result.len() as u64 >= MAX_FILES || total > MAX_TOTAL_BYTES {
            return Err(invalid("归档附件数量或总大小超过限制"));
        }
        let (kind, _) = extension_kind(&name);
        result.push(ArchiveFileInfo {
            relative_name: name,
            size_bytes: size,
            sha256: hash,
            kind: kind.into(),
        });
    }
    Ok(result)
}

pub fn preview(
    conn: &Connection,
    page_uuid: &str,
    relative_name: &str,
) -> AppResult<ArchiveFilePreview> {
    let (kind, mime_type) = extension_kind(relative_name);
    if kind != "image" {
        return Err(invalid(
            "只允许预览受限图片附件；HTML、SVG 和其他文件必须导出后处理",
        ));
    }
    let (bytes, sha256) = read_content(conn, page_uuid, relative_name, MAX_PREVIEW_BYTES)?;
    if !image_magic_matches(relative_name, &bytes) {
        return Err(invalid("图片附件内容与扩展名不匹配，拒绝预览"));
    }
    Ok(ArchiveFilePreview {
        relative_name: relative_name.into(),
        mime_type: mime_type.into(),
        bytes,
        sha256,
    })
}

pub fn export_checked<F: Fn() -> AppResult<()>>(
    conn: &Connection,
    page_uuid: &str,
    relative_name: &str,
    target_path: &Path,
    check: F,
) -> AppResult<()> {
    check()?;
    let (bytes, _) = read_content(conn, page_uuid, relative_name, MAX_FILE_BYTES)?;
    check()?;
    if target_path.as_os_str().is_empty() || !target_path.is_absolute() {
        return Err(invalid("导出目标不能为空"));
    }
    if let Ok(metadata) = fs::symlink_metadata(target_path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() || is_reparse_point(&metadata) {
            return Err(invalid("导出目标不是普通文件，拒绝覆盖"));
        }
    }
    let parent = target_path
        .parent()
        .ok_or_else(|| invalid("导出目标目录无效"))?;
    let parent_metadata = fs::symlink_metadata(parent).map_err(AppError::from)?;
    if !parent_metadata.is_dir()
        || parent_metadata.file_type().is_symlink()
        || is_reparse_point(&parent_metadata)
    {
        return Err(invalid("导出目标目录无效"));
    }
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target_path)?;
    if let Err(error) = output.write_all(&bytes).and_then(|_| output.sync_all()) {
        drop(output);
        let _ = fs::remove_file(target_path);
        return Err(error.into());
    }
    if let Err(error) = check() {
        drop(output);
        let _ = fs::remove_file(target_path);
        return Err(error);
    }
    drop(output);
    Ok(())
}

#[cfg(windows)]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
}

pub fn export(
    conn: &Connection,
    page_uuid: &str,
    relative_name: &str,
    target_path: &Path,
) -> AppResult<()> {
    export_checked(conn, page_uuid, relative_name, target_path, || Ok(()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn fixture() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE legacy_archive_files(page_uuid TEXT,relative_name TEXT,sha256 TEXT,size_bytes INTEGER,content BLOB,PRIMARY KEY(page_uuid,relative_name));").unwrap();
        let uuid = "123e4567-e89b-12d3-a456-426614174000";
        let bytes: &[u8] = &[
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9c, 0x63, 0x60, 0x60, 0x60, 0x60, 0x00, 0x00, 0x00, 0x05, 0x00, 0x01, 0xa5, 0xf6,
            0x27, 0x9a, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ];
        conn.execute(
            "INSERT INTO legacy_archive_files VALUES(?1,'img.png',?2,?3,?4)",
            rusqlite::params![
                uuid,
                format!("{:x}", Sha256::digest(bytes)),
                bytes.len() as i64,
                bytes
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO legacy_archive_files VALUES(?1,'note.txt',?2,?3,?4)",
            rusqlite::params![
                uuid,
                format!("{:x}", Sha256::digest(b"note")),
                4i64,
                b"note"
            ],
        )
        .unwrap();
        conn
    }

    #[test]
    fn list_validates_and_preview_restricts_types() {
        let conn = fixture();
        assert_eq!(
            list(&conn, "123e4567-e89b-12d3-a456-426614174000")
                .unwrap()
                .len(),
            2
        );
        assert!(preview(&conn, "123e4567-e89b-12d3-a456-426614174000", "img.png").is_ok());
        assert!(preview(&conn, "123e4567-e89b-12d3-a456-426614174000", "note.txt").is_err());
        assert!(list(&conn, "../outside").is_err());
    }

    #[test]
    fn tampering_and_oversized_content_are_rejected_before_materialization() {
        let conn = fixture();
        conn.execute(
            "UPDATE legacy_archive_files SET sha256='00' WHERE relative_name='img.png'",
            [],
        )
        .unwrap();
        assert!(preview(&conn, "123e4567-e89b-12d3-a456-426614174000", "img.png").is_err());
    }

    #[test]
    fn export_is_create_new_and_cleans_only_its_new_output_on_cancellation() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let conn = fixture();
        let root = std::env::temp_dir().join(format!("browsory_archive_export_{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let target = root.join("image.png");
        export(
            &conn,
            "123e4567-e89b-12d3-a456-426614174000",
            "img.png",
            &target,
        )
        .unwrap();
        assert_eq!(fs::read(&target).unwrap().len(), 70);
        fs::write(&target, b"keep this file").unwrap();
        assert!(export(
            &conn,
            "123e4567-e89b-12d3-a456-426614174000",
            "img.png",
            &target
        )
        .is_err());
        assert_eq!(fs::read(&target).unwrap(), b"keep this file");

        let before = root.join("cancel-before.png");
        assert!(export_checked(
            &conn,
            "123e4567-e89b-12d3-a456-426614174000",
            "img.png",
            &before,
            || Err(invalid("cancelled"))
        )
        .is_err());
        assert!(!before.exists());
        let after = root.join("cancel-after.png");
        let checks = AtomicUsize::new(0);
        assert!(export_checked(
            &conn,
            "123e4567-e89b-12d3-a456-426614174000",
            "img.png",
            &after,
            || {
                if checks.fetch_add(1, Ordering::AcqRel) >= 2 {
                    Err(invalid("cancelled"))
                } else {
                    Ok(())
                }
            }
        )
        .is_err());
        assert!(!after.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn preview_rejects_extension_spoof_and_declared_length_tampering() {
        let conn = fixture();
        let uuid = "123e4567-e89b-12d3-a456-426614174000";
        conn.execute("UPDATE legacy_archive_files SET relative_name='fake.jpg' WHERE relative_name='img.png'", []).unwrap();
        assert!(preview(&conn, uuid, "fake.jpg").is_err());
        conn.execute("UPDATE legacy_archive_files SET relative_name='img.png', size_bytes=size_bytes+1 WHERE relative_name='fake.jpg'", []).unwrap();
        assert!(preview(&conn, uuid, "img.png").is_err());
    }
}
