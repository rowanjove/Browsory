//! Import a complete bounded legacy directory without deleting any source file.
use super::{compress_html, validate_uuid, MAX_HTML_BYTES, MAX_MARKDOWN_BYTES};
use crate::error::{AppError, AppResult};
use crate::security::fs_boundary::{collect_scoped_files, read_scoped_file, resolve_scoped_path};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::path::{Component, Path};

const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
const MAX_FILES: usize = 128;

#[derive(zeroize::Zeroize, zeroize::ZeroizeOnDrop)]
pub(super) struct LegacyFile {
    pub(super) name: String,
    pub(super) bytes: Vec<u8>,
    pub(super) sha256: String,
}

fn invalid(message: &str) -> AppError {
    AppError::Security(message.into())
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn read_page<F>(root: &Path, relative: &Path, check: &F) -> AppResult<Vec<LegacyFile>>
where
    F: Fn() -> AppResult<()>,
{
    let mut files = Vec::new();
    let mut total = 0u64;
    for path in collect_scoped_files(root, relative, MAX_FILES, 8)? {
        check()?;
        let name = path
            .strip_prefix(relative)
            .map_err(|_| invalid("旧归档路径超出页面目录"))?
            .components()
            .map(|part| match part {
                Component::Normal(value) => value
                    .to_str()
                    .ok_or_else(|| invalid("旧归档文件名不是 UTF-8")),
                _ => Err(invalid("旧归档文件名包含非法路径组件")),
            })
            .collect::<AppResult<Vec<_>>>()?
            .join("/");
        let limit = match name.as_str() {
            "content.md" => MAX_MARKDOWN_BYTES,
            "snapshot.html" => MAX_HTML_BYTES,
            _ => MAX_FILE_BYTES,
        };
        let bytes = read_scoped_file(root, &path, limit.min(MAX_TOTAL_BYTES - total))?
            .ok_or_else(|| invalid("旧归档文件在迁移时消失，保留源目录"))?;
        total = total
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| invalid("旧归档大小溢出"))?;
        if total > MAX_TOTAL_BYTES {
            return Err(invalid("单页旧归档总大小超过 64 MiB，保留源目录"));
        }
        files.push(LegacyFile {
            name,
            sha256: hash(&bytes),
            bytes,
        });
    }
    files.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(files)
}

pub(super) fn manifest(files: &[LegacyFile]) -> String {
    let mut hasher = Sha256::new();
    for file in files {
        hasher.update((file.name.len() as u64).to_le_bytes());
        hasher.update(file.name.as_bytes());
        hasher.update((file.bytes.len() as u64).to_le_bytes());
        hasher.update(file.sha256.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn verify_stored(conn: &Connection, uuid: &str, files: &[LegacyFile]) -> AppResult<()> {
    let count: usize = conn.query_row(
        "SELECT count(*) FROM legacy_archive_files WHERE page_uuid=?1",
        [uuid],
        |r| r.get(0),
    )?;
    if count != files.len() {
        return Err(invalid("旧归档加密文件数量不符，保留源目录"));
    }
    for file in files {
        let (length, declared, digest): (u64, u64, String) = conn.query_row(
            "SELECT length(content),size_bytes,sha256 FROM legacy_archive_files WHERE page_uuid=?1 AND relative_name=?2",
            params![uuid,file.name], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        if length != file.bytes.len() as u64 || declared != length || digest != file.sha256 {
            return Err(invalid("旧归档加密文件长度或哈希记录不符，保留源目录"));
        }
        let stored: Vec<u8> = conn.query_row(
            "SELECT content FROM legacy_archive_files WHERE page_uuid=?1 AND relative_name=?2",
            params![uuid, file.name],
            |r| r.get(0),
        )?;
        if hash(&stored) != file.sha256 {
            return Err(invalid("旧归档加密内容哈希不符，保留源目录"));
        }
    }
    Ok(())
}

/// Read and verify a whole retained page, including its migration receipt.
/// This also works after the active reading-list entry has been deleted.
pub(super) fn read_stored_page(conn: &Connection, uuid: &str) -> AppResult<Vec<LegacyFile>> {
    validate_uuid(uuid)?;
    let mut statement=conn.prepare("SELECT relative_name,size_bytes,length(content),sha256 FROM legacy_archive_files WHERE page_uuid=?1 ORDER BY relative_name LIMIT 129")?;
    let mut rows = statement.query([uuid])?;
    let mut files = Vec::new();
    let mut total = 0u64;
    while let Some(row) = rows.next()? {
        if row
            .get_ref(0)?
            .as_str()
            .map_err(|_| invalid("旧归档文件名类型无效"))?
            .len()
            > 8192
            || row
                .get_ref(3)?
                .as_str()
                .map_err(|_| invalid("旧归档哈希类型无效"))?
                .len()
                != 64
        {
            return Err(invalid("加密旧归档名称或哈希长度无效"));
        }
        let name: String = row.get(0)?;
        let size: u64 = row.get(1)?;
        let length: u64 = row.get(2)?;
        let digest: String = row.get(3)?;
        if name.is_empty()
            || name.contains(['\\', '\0', ':'])
            || Path::new(&name)
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
            || size != length
            || size > MAX_FILE_BYTES
            || files.len() >= MAX_FILES
        {
            return Err(invalid("加密旧归档文件清单无效"));
        }
        total = total
            .checked_add(size)
            .ok_or_else(|| invalid("旧归档大小溢出"))?;
        if total > MAX_TOTAL_BYTES {
            return Err(invalid("旧归档超过单页限额"));
        }
        let bytes: Vec<u8> = conn.query_row(
            "SELECT content FROM legacy_archive_files WHERE page_uuid=?1 AND relative_name=?2",
            params![uuid, name],
            |r| r.get(0),
        )?;
        if hash(&bytes) != digest {
            return Err(invalid("加密旧归档内容哈希不符"));
        }
        files.push(LegacyFile {
            name,
            bytes,
            sha256: digest,
        });
    }
    let (declared,count,size):(String,usize,u64)=conn.query_row("SELECT manifest_sha256,file_count,total_bytes FROM legacy_archive_migrations WHERE page_uuid=?1",[uuid],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    if files.is_empty() || declared != manifest(&files) || count != files.len() || size != total {
        return Err(invalid("旧归档迁移凭据与加密内容不符"));
    }
    Ok(files)
}

fn import_page<F, O>(
    root: &Path,
    conn: &Connection,
    uuid: &str,
    check: &F,
    observe: O,
) -> AppResult<bool>
where
    F: Fn() -> AppResult<()>,
    O: FnOnce() -> AppResult<()>,
{
    validate_uuid(uuid)?;
    let relative = Path::new("archive").join("pages").join(uuid);
    let files = read_page(root, &relative, check)?;
    if files.is_empty() {
        return Ok(false);
    }
    let digest = manifest(&files);
    let previous: Option<String> = conn
        .query_row(
            "SELECT manifest_sha256 FROM legacy_archive_migrations WHERE page_uuid=?1",
            [uuid],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(previous) = previous {
        if previous != digest {
            return Err(invalid(
                "已迁移的旧归档源目录发生变化，保留全部材料，禁止静默覆盖",
            ));
        }
        verify_stored(conn, uuid, &files)?;
        return Ok(false);
    }
    let tx = conn.unchecked_transaction()?;
    // Unknown schema/data errors must roll back this page, never count as imported.
    for file in &files {
        tx.execute("INSERT INTO legacy_archive_files(page_uuid,relative_name,sha256,size_bytes,content) VALUES(?1,?2,?3,?4,?5)",
            params![uuid,file.name,file.sha256,file.bytes.len() as u64,file.bytes])?;
    }
    let markdown = files
        .iter()
        .find(|file| file.name == "content.md")
        .map(|file| {
            std::str::from_utf8(&file.bytes)
                .map_err(|_| invalid("旧 Markdown 并非有效 UTF-8，保留源文件"))
        })
        .transpose()?;
    let html = files
        .iter()
        .find(|file| file.name == "snapshot.html")
        .map(|file| {
            std::str::from_utf8(&file.bytes)
                .map_err(|_| invalid("旧 HTML 并非有效 UTF-8，保留源文件"))
        })
        .transpose()?;
    let compressed = html.map(compress_html).transpose()?;
    let has_screenshot = files.iter().any(|file| {
        let name = file.name.to_ascii_lowercase();
        name.ends_with(".png")
            || name.ends_with(".jpg")
            || name.ends_with(".jpeg")
            || name.ends_with(".webp")
    });
    let changed = tx.execute("UPDATE offline_archives SET markdown_content=COALESCE(markdown_content,?1),
        snapshot_html_compressed=COALESCE(snapshot_html_compressed,?2),
        has_markdown=CASE WHEN markdown_content IS NOT NULL OR ?1 IS NOT NULL THEN 1 ELSE has_markdown END,
        has_snapshot_html=CASE WHEN snapshot_html_compressed IS NOT NULL OR ?2 IS NOT NULL THEN 1 ELSE has_snapshot_html END,
        has_screenshot=CASE WHEN ?3 THEN 1 ELSE has_screenshot END WHERE page_uuid=?4",
        params![markdown,compressed,has_screenshot,uuid])?;
    if changed != 1 {
        return Err(invalid("旧归档没有唯一对应数据库记录，保留源目录"));
    }
    verify_stored(&tx, uuid, &files)?;
    observe()?;
    // Detect source additions, removals and content changes before committing.
    let refreshed = read_page(root, &relative, check)?;
    if manifest(&refreshed) != digest {
        return Err(invalid("旧归档源文件在迁移期间变化，已回滚并保留源目录"));
    }
    check()?;
    let total: u64 = files.iter().map(|file| file.bytes.len() as u64).sum();
    tx.execute("INSERT INTO legacy_archive_migrations(page_uuid,manifest_sha256,file_count,total_bytes,verified_at) VALUES(?1,?2,?3,?4,?5)",
        params![uuid,digest,files.len() as u64,total,chrono::Utc::now().timestamp_millis()])?;
    tx.commit()?;
    Ok(true)
}

pub fn migrate_legacy_disk_archives(root: &Path, conn: &Connection) -> AppResult<usize> {
    migrate_legacy_disk_archives_checked(root, conn, || Ok(()))
}

pub fn migrate_legacy_disk_archives_checked<F>(
    root: &Path,
    conn: &Connection,
    check: F,
) -> AppResult<usize>
where
    F: Fn() -> AppResult<()>,
{
    let Some(base) = resolve_scoped_path(root, Path::new("archive/pages"))? else {
        return Ok(0);
    };
    let mut count = 0;
    for entry in std::fs::read_dir(base)? {
        check()?;
        let entry = entry?;
        let Some(uuid) = entry.file_name().to_str().map(str::to_string) else {
            return Err(invalid("旧归档目录名不是 UTF-8，保留现场"));
        };
        if validate_uuid(&uuid).is_err() {
            continue;
        }
        let relative = Path::new("archive").join("pages").join(&uuid);
        let Some(path) = resolve_scoped_path(root, &relative)? else {
            return Err(invalid("旧归档目录在迁移期间消失"));
        };
        if !path.is_dir() {
            return Err(invalid("旧归档 UUID 对象不是目录，保留现场"));
        }
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM offline_archives WHERE page_uuid=?1)",
            [&uuid],
            |r| r.get(0),
        )?;
        if !exists {
            let verified: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM legacy_archive_migrations WHERE page_uuid=?1)",
                [&uuid],
                |r| r.get(0),
            )?;
            if !verified {
                return Err(invalid(
                    "旧归档目录没有对应数据库记录或加密迁移凭据，保留源文件并停止迁移",
                ));
            }
            // Removing a reading-list entry does not discard the verified
            // recovery copy of a retained plaintext source directory.
            import_page(root, conn, &uuid, &check, || Ok(()))?;
            continue;
        }
        if import_page(root, conn, &uuid, &check, || Ok(()))? {
            count += 1;
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::vault::VaultManager;
    use std::fs;

    fn fixture() -> (std::path::PathBuf, VaultManager, String) {
        let root =
            std::env::temp_dir().join(format!("browsory_legacy_archive_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let vault = VaultManager::unlocked_fixture(root.clone());
        let uuid = uuid::Uuid::new_v4().to_string();
        vault.with_conn(|conn| {
            conn.execute("INSERT INTO urls(id,url,title,domain) VALUES(1,'https://archive.test','Old title','archive.test')", [])?;
            conn.execute("INSERT INTO offline_archives(url_id,page_uuid,relative_path,has_markdown,has_snapshot_html,archived_at) VALUES(1,?1,?2,1,1,0)", params![uuid,format!("pages/{uuid}")])?;
            Ok(())
        }).unwrap();
        let page = root.join("archive/pages").join(&uuid);
        fs::create_dir_all(page.join("assets")).unwrap();
        fs::write(page.join("content.md"), b"PRIVATE-LEGACY-ARCHIVE-CONTENT").unwrap();
        fs::write(
            page.join("snapshot.html"),
            b"<html>PRIVATE-LEGACY-ARCHIVE-CONTENT</html>",
        )
        .unwrap();
        fs::write(
            page.join("metadata.json"),
            b"{\"title\":\"keep exact original metadata\"}",
        )
        .unwrap();
        fs::write(
            page.join("assets/screenshot.png"),
            [137, 80, 78, 71, 0, 255],
        )
        .unwrap();
        fs::write(page.join("assets/unknown.bin"), [0, 1, 255, 0, 2]).unwrap();
        fs::write(page.join("empty.txt"), []).unwrap();
        (root, vault, uuid)
    }

    #[test]
    fn all_legacy_files_are_verified_encrypted_idempotent_and_sources_are_retained() {
        let (root, vault, uuid) = fixture();
        vault.with_conn(|conn| {
            assert_eq!(migrate_legacy_disk_archives(&root, conn)?, 1);
            assert_eq!(migrate_legacy_disk_archives(&root, conn)?, 0);
            let files = read_page(&root, &Path::new("archive/pages").join(&uuid), &|| Ok(()))?;
            assert_eq!(files.len(), 6); verify_stored(conn, &uuid, &files)?;
            let receipt: String = conn.query_row("SELECT manifest_sha256 FROM legacy_archive_migrations WHERE page_uuid=?1", [&uuid], |r| r.get(0))?;
            assert_eq!(receipt, manifest(&files));
            let detail = super::super::get_offline_archive(&root, conn, 1)?.unwrap();
            assert_eq!(detail.markdown_content.as_deref(), Some("PRIVATE-LEGACY-ARCHIVE-CONTENT"));
            assert!(detail.summary.has_screenshot);
            conn.execute("UPDATE offline_archives SET markdown_content='newer encrypted edit' WHERE page_uuid=?1", [&uuid])?;
            assert_eq!(migrate_legacy_disk_archives(&root, conn)?, 0);
            assert_eq!(super::super::get_offline_archive(&root, conn, 1)?.unwrap().markdown_content.as_deref(), Some("newer encrypted edit"));
            super::super::delete_offline_archive(&root, conn, &uuid)?;
            assert!(super::super::get_offline_archive(&root, conn, 1)?.is_none());
            assert_eq!(migrate_legacy_disk_archives(&root, conn)?, 0);
            verify_stored(conn, &uuid, &files)?;
            let backup = crate::security::backup::execute_encrypted_backup(conn, &root, &root.join("backups"),
                "legacy", None, &vault.master_key().unwrap(), &vault.envelope().unwrap()).map_err(AppError::Security)?;
            assert!(Path::new(&backup.file_path).is_file());
            Ok(())
        }).unwrap();
        vault.lock().unwrap();
        let bytes = fs::read(root.join("archive.db")).unwrap();
        assert!(!bytes
            .windows(b"PRIVATE-LEGACY-ARCHIVE-CONTENT".len())
            .any(|window| window == b"PRIVATE-LEGACY-ARCHIVE-CONTENT"));
        assert!(root
            .join("archive/pages")
            .join(&uuid)
            .join("assets/unknown.bin")
            .exists());
        drop(vault);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn storage_corruption_and_source_change_roll_back_without_deleting_anything() {
        let (root, vault, uuid) = fixture();
        vault.with_conn(|conn| {
            conn.execute_batch("CREATE TRIGGER damage_import AFTER INSERT ON legacy_archive_files BEGIN UPDATE legacy_archive_files SET content=X'00' WHERE page_uuid=NEW.page_uuid AND relative_name=NEW.relative_name; END;")?;
            assert!(migrate_legacy_disk_archives(&root, conn).is_err());
            assert_eq!(conn.query_row("SELECT count(*) FROM legacy_archive_files", [], |r| r.get::<_,i64>(0))?, 0);
            assert_eq!(conn.query_row("SELECT count(*) FROM legacy_archive_migrations", [], |r| r.get::<_,i64>(0))?, 0);
            conn.execute_batch("DROP TRIGGER damage_import")?;
            let page = root.join("archive/pages").join(&uuid);
            assert!(import_page(&root, conn, &uuid, &|| Ok(()), || { fs::write(page.join("content.md"), b"source changed")?; Ok(()) }).is_err());
            assert_eq!(conn.query_row("SELECT count(*) FROM legacy_archive_files", [], |r| r.get::<_,i64>(0))?, 0);
            assert_eq!(fs::read(page.join("content.md"))?, b"source changed");
            assert!(page.join("assets/screenshot.png").exists());
            Ok(())
        }).unwrap();
        vault.lock().unwrap();
        drop(vault);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_text_and_cancelled_work_preserve_sources_and_block_backup() {
        let (root, vault, uuid) = fixture();
        let page = root.join("archive/pages").join(&uuid);
        fs::write(page.join("content.md"), [255, 254]).unwrap();
        vault
            .with_conn(|conn| {
                assert!(migrate_legacy_disk_archives(&root, conn).is_err());
                assert!(
                    migrate_legacy_disk_archives_checked(&root, conn, || Err(invalid("cancelled")))
                        .is_err()
                );
                assert_eq!(
                    conn.query_row("SELECT count(*) FROM legacy_archive_files", [], |r| r
                        .get::<_, i64>(0))?,
                    0
                );
                Ok(())
            })
            .unwrap();
        let state = crate::database::DbState {
            _directory_lock: None,
            vault: std::sync::Arc::new(vault),
            app_dir: root.clone(),
            temp_dir: root.join("temp"),
            logs_dir: root.join("logs"),
        };
        state
            .with_conn(|conn| {
                assert!(crate::commands::security::execute_backup_internal(
                    conn,
                    &state,
                    &root.join("backups"),
                    "test",
                    None
                )
                .is_err());
                Ok(())
            })
            .unwrap();
        assert!(!root.join("backups").exists());
        assert_eq!(fs::read(page.join("content.md")).unwrap(), [255, 254]);
        state.vault.lock().unwrap();
        drop(state);
        fs::remove_dir_all(root).unwrap();
    }
}
