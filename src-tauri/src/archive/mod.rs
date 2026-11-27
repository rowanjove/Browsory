use chrono::Utc;
use flate2::write::DeflateEncoder;
use flate2::{Compression, Decompress, FlushDecompress, Status};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::error::{AppError, AppResult};

pub mod attachments;
mod legacy;
pub mod retirement;
pub use legacy::{migrate_legacy_disk_archives, migrate_legacy_disk_archives_checked};

pub const MAX_MARKDOWN_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_HTML_BYTES: u64 = 16 * 1024 * 1024;
const MAX_COMPRESSED_HTML_BYTES: u64 = MAX_HTML_BYTES + 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageArchiveSummary {
    pub id: i64,
    pub url_id: i64,
    pub page_uuid: String,
    pub url: String,
    pub title: String,
    pub domain: String,
    pub archive_level: String,
    pub relative_path: String,
    pub has_markdown: bool,
    pub has_snapshot_html: bool,
    pub has_screenshot: bool,
    pub size_bytes: u64,
    pub archived_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageArchiveDetail {
    pub summary: PageArchiveSummary,
    pub markdown_content: Option<String>,
    pub snapshot_html: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveArchivePayload {
    pub url_id: i64,
    pub archive_level: String,
    pub markdown: Option<String>,
    pub html: Option<String>,
    pub title: Option<String>,
    pub author: Option<String>,
}

pub fn get_archive_base_dir(app_dir: &Path) -> PathBuf {
    app_dir.join("archive").join("pages")
}

pub fn compress_html(html: &str) -> AppResult<Vec<u8>> {
    if html.len() as u64 > MAX_HTML_BYTES {
        return Err(AppError::Security("离线 HTML 超出 16 MiB 限制".into()));
    }
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(html.as_bytes())?;
    Ok(encoder.finish()?)
}

pub fn decompress_html(bytes: &[u8]) -> AppResult<String> {
    if bytes.len() as u64 > MAX_COMPRESSED_HTML_BYTES {
        return Err(AppError::Security("离线压缩 HTML 超出限制".into()));
    }
    let mut result = Vec::new();
    let mut decoder = Decompress::new(false);
    let mut output = [0u8; 64 * 1024];
    loop {
        let before_in = decoder.total_in();
        let before_out = decoder.total_out();
        let status = decoder
            .decompress(
                &bytes[before_in as usize..],
                &mut output,
                FlushDecompress::None,
            )
            .map_err(|_| AppError::Parse("离线压缩 HTML 损坏".into()))?;
        if decoder.total_out() > MAX_HTML_BYTES {
            return Err(AppError::Security("离线 HTML 解压超出 16 MiB 限制".into()));
        }
        result.extend_from_slice(&output[..(decoder.total_out() - before_out) as usize]);
        if status == Status::StreamEnd {
            if decoder.total_in() != bytes.len() as u64 {
                return Err(AppError::Parse("离线压缩 HTML 包含多余数据".into()));
            }
            break;
        }
        if decoder.total_in() == before_in && decoder.total_out() == before_out {
            return Err(AppError::Parse("离线压缩 HTML 被截断或无效".into()));
        }
    }
    String::from_utf8(result).map_err(|_| AppError::Parse("离线 HTML 并非有效 UTF-8".into()))
}

fn validate_uuid(value: &str) -> AppResult<()> {
    if Uuid::parse_str(value)
        .map(|uuid| uuid.to_string() == value)
        .unwrap_or(false)
    {
        Ok(())
    } else {
        Err(AppError::Security("归档标识格式无效".into()))
    }
}

fn read_legacy_text(
    app_dir: &Path,
    uuid: &str,
    name: &str,
    limit: u64,
) -> AppResult<Option<String>> {
    validate_uuid(uuid)?;
    let relative = Path::new("archive").join("pages").join(uuid).join(name);
    crate::security::fs_boundary::read_scoped_file(app_dir, &relative, limit)?
        .map(|bytes| {
            String::from_utf8(bytes)
                .map_err(|_| AppError::Parse("旧归档文本并非有效 UTF-8，保留源文件".into()))
        })
        .transpose()
}

pub fn save_offline_archive(
    _app_dir: &Path,
    conn: &Connection,
    payload: SaveArchivePayload,
) -> AppResult<PageArchiveSummary> {
    if payload
        .markdown
        .as_ref()
        .is_some_and(|text| text.len() as u64 > MAX_MARKDOWN_BYTES)
    {
        return Err(AppError::Security("离线 Markdown 超出 8 MiB 限制".into()));
    }
    if payload
        .html
        .as_ref()
        .is_some_and(|text| text.len() as u64 > MAX_HTML_BYTES)
    {
        return Err(AppError::Security("离线 HTML 超出 16 MiB 限制".into()));
    }
    let tx = conn.unchecked_transaction()?;
    let conn = &tx;
    // 1. Fetch url info from urls table
    let (db_url, db_title, db_domain): (String, String, String) = conn.query_row(
        "SELECT url, COALESCE(title, ''), COALESCE(domain, '') FROM urls WHERE id = ?1",
        params![payload.url_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;

    let title = payload
        .title
        .filter(|t| !t.trim().is_empty())
        .unwrap_or(db_title);
    let domain = db_domain;
    let url = db_url;

    // Check if an archive already exists for this url_id
    let existing_uuid: Option<String> = conn
        .query_row(
            "SELECT page_uuid FROM offline_archives WHERE url_id = ?1",
            params![payload.url_id],
            |r| r.get(0),
        )
        .optional()?;

    let page_uuid = existing_uuid.unwrap_or_else(|| Uuid::new_v4().to_string());
    validate_uuid(&page_uuid)?;
    let relative_path = format!("pages/{}", page_uuid);

    let mut size_bytes = 0u64;
    let markdown_val = payload.markdown.filter(|value| !value.trim().is_empty());
    let has_markdown = markdown_val.is_some();

    let html_val = payload.html.filter(|value| !value.trim().is_empty());
    let has_snapshot_html = html_val.is_some();

    if let Some(ref md) = markdown_val {
        size_bytes += md.len() as u64;

        // Upsert into page_contents table for FTS5 full-text indexing
        let now = Utc::now().timestamp_millis();
        let word_count = md.split_whitespace().count() as i64;
        let author = payload.author.clone().unwrap_or_default();
        conn.execute(
            r#"
            INSERT INTO page_contents (url_id, title, author, description, markdown, plain_text, word_count, extracted_at)
            VALUES (?1, ?2, ?3, '', ?4, ?4, ?5, ?6)
            ON CONFLICT(url_id) DO UPDATE SET
                title = excluded.title,
                author = excluded.author,
                markdown = excluded.markdown,
                plain_text = excluded.plain_text,
                word_count = excluded.word_count,
                extracted_at = excluded.extracted_at
            "#,
            params![payload.url_id, title, author, md, word_count, now],
        )?;
    } else {
        // A metadata-only save keeps an existing body and its search index.
        conn.execute(
            "UPDATE page_contents SET title=?1 WHERE url_id=?2",
            params![title, payload.url_id],
        )?;
    }

    let compressed_html = html_val.as_deref().map(compress_html).transpose()?;
    if let Some(ref comp) = compressed_html {
        size_bytes += comp.len() as u64;
    }

    let now = Utc::now().timestamp_millis();

    // Upsert into offline_archives table directly storing encrypted content in SQLCipher
    let record_id: i64 = conn.query_row(
        r#"
        INSERT INTO offline_archives (
            url_id, page_uuid, archive_level, relative_path, title, domain,
            has_markdown, has_snapshot_html, has_screenshot, size_bytes, archived_at,
            markdown_content, snapshot_html_compressed
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9, ?10, ?11, ?12)
        ON CONFLICT(page_uuid) DO UPDATE SET
            archive_level = excluded.archive_level,
            title = excluded.title,
            domain = excluded.domain,
            has_markdown = CASE WHEN excluded.markdown_content IS NOT NULL THEN 1 ELSE offline_archives.has_markdown END,
            has_snapshot_html = CASE WHEN excluded.snapshot_html_compressed IS NOT NULL THEN 1 ELSE offline_archives.has_snapshot_html END,
            size_bytes = excluded.size_bytes,
            archived_at = excluded.archived_at,
            markdown_content = COALESCE(excluded.markdown_content,offline_archives.markdown_content),
            snapshot_html_compressed = COALESCE(excluded.snapshot_html_compressed,offline_archives.snapshot_html_compressed)
        RETURNING id
        "#,
        params![
            payload.url_id,
            page_uuid,
            payload.archive_level,
            relative_path,
            title,
            domain,
            has_markdown as i32,
            has_snapshot_html as i32,
            size_bytes as i64,
            now,
            markdown_val,
            compressed_html
        ],
        |r| r.get(0),
    )?;

    // Keep retained legacy sources. Saving a new version must never erase
    // screenshots, metadata or unknown attachments from an older directory.
    conn.execute("UPDATE offline_archives SET size_bytes=COALESCE(length(CAST(markdown_content AS BLOB)),0)+COALESCE(length(snapshot_html_compressed),0) WHERE id=?1", [record_id])?;
    let (has_markdown,has_snapshot_html,has_screenshot,size_bytes): (bool,bool,bool,u64) = conn.query_row(
        "SELECT has_markdown,has_snapshot_html,has_screenshot,size_bytes FROM offline_archives WHERE id=?1",
        [record_id],
        |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
    )?;
    tx.commit()?;

    Ok(PageArchiveSummary {
        id: record_id,
        url_id: payload.url_id,
        page_uuid,
        url,
        title,
        domain,
        archive_level: payload.archive_level,
        relative_path,
        has_markdown,
        has_snapshot_html,
        has_screenshot,
        size_bytes,
        archived_at: now,
    })
}

pub fn get_offline_archive(
    app_dir: &Path,
    conn: &Connection,
    url_id: i64,
) -> AppResult<Option<PageArchiveDetail>> {
    let result = conn.query_row(
        r#"SELECT oa.id, oa.url_id, oa.page_uuid, u.url, COALESCE(oa.title,''), COALESCE(oa.domain,''),
        oa.archive_level, oa.relative_path, oa.has_markdown, oa.has_snapshot_html, oa.has_screenshot,
        oa.size_bytes, oa.archived_at, oa.markdown_content, oa.snapshot_html_compressed,
        length(CAST(oa.markdown_content AS BLOB)), length(oa.snapshot_html_compressed)
        FROM offline_archives oa JOIN urls u ON oa.url_id=u.id WHERE oa.url_id=?1"#,
        [url_id], |r| {
            let markdown_len: Option<u64> = r.get(15)?;
            let html_len: Option<u64> = r.get(16)?;
            if markdown_len.unwrap_or(0) > MAX_MARKDOWN_BYTES || html_len.unwrap_or(0) > MAX_COMPRESSED_HTML_BYTES {
                return Err(rusqlite::Error::InvalidQuery);
            }
            Ok((PageArchiveSummary { id:r.get(0)?, url_id:r.get(1)?, page_uuid:r.get(2)?, url:r.get(3)?,
                title:r.get(4)?, domain:r.get(5)?, archive_level:r.get(6)?, relative_path:r.get(7)?,
                has_markdown:r.get::<_,i32>(8)? != 0, has_snapshot_html:r.get::<_,i32>(9)? != 0,
                has_screenshot:r.get::<_,i32>(10)? != 0, size_bytes:r.get::<_,u64>(11)?, archived_at:r.get(12)? },
                r.get::<_, Option<String>>(13)?, r.get::<_, Option<Vec<u8>>>(14)?))
        }).optional()?;
    let Some((summary, db_markdown, db_html)) = result else {
        return Ok(None);
    };
    validate_uuid(&summary.page_uuid)?;
    let markdown_content = match db_markdown {
        Some(text) => Some(text),
        None if summary.has_markdown => read_legacy_text(
            app_dir,
            &summary.page_uuid,
            "content.md",
            MAX_MARKDOWN_BYTES,
        )?,
        None => None,
    };
    let snapshot_html = match db_html {
        Some(bytes) => Some(decompress_html(&bytes)?),
        None if summary.has_snapshot_html => {
            read_legacy_text(app_dir, &summary.page_uuid, "snapshot.html", MAX_HTML_BYTES)?
        }
        None => None,
    };
    Ok(Some(PageArchiveDetail {
        summary,
        markdown_content,
        snapshot_html,
    }))
}

pub fn list_offline_archives(
    conn: &Connection,
    limit: u32,
    offset: u32,
) -> AppResult<Vec<PageArchiveSummary>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT oa.id, oa.url_id, oa.page_uuid, u.url, oa.title, oa.domain,
               oa.archive_level, oa.relative_path, oa.has_markdown,
               oa.has_snapshot_html, oa.has_screenshot, oa.size_bytes, oa.archived_at
        FROM offline_archives oa
        JOIN urls u ON oa.url_id = u.id
        ORDER BY oa.archived_at DESC
        LIMIT ?1 OFFSET ?2
        "#,
    )?;

    let rows = stmt.query_map(params![limit, offset], |r| {
        Ok(PageArchiveSummary {
            id: r.get(0)?,
            url_id: r.get(1)?,
            page_uuid: r.get(2)?,
            url: r.get(3)?,
            title: r.get(4)?,
            domain: r.get(5)?,
            archive_level: r.get(6)?,
            relative_path: r.get(7)?,
            has_markdown: r.get::<_, i32>(8)? != 0,
            has_snapshot_html: r.get::<_, i32>(9)? != 0,
            has_screenshot: r.get::<_, i32>(10)? != 0,
            size_bytes: r.get::<_, i64>(11)? as u64,
            archived_at: r.get(12)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(AppError::Database)
}

pub fn delete_offline_archive(
    _app_dir: &Path,
    conn: &Connection,
    page_uuid: &str,
) -> AppResult<()> {
    validate_uuid(page_uuid)?;
    // Remove the reading entry only. Verified legacy recovery copies and their
    // source directories need a separate explicit retirement workflow.
    conn.execute(
        "DELETE FROM offline_archives WHERE page_uuid=?1",
        [page_uuid],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn payload(markdown: &str) -> SaveArchivePayload {
        SaveArchivePayload {
            url_id: 1,
            archive_level: "full".into(),
            markdown: Some(markdown.into()),
            html: Some("<html>original HTML</html>".into()),
            title: Some("Original title".into()),
            author: None,
        }
    }

    #[test]
    fn archive_and_search_writes_are_atomic_and_never_delete_retained_sources() {
        let root = std::env::temp_dir().join(format!("browsory_archive_atomic_{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let vault = crate::security::vault::VaultManager::unlocked_fixture(root.clone());
        vault.with_conn(|conn| {
            conn.execute("INSERT INTO urls(id,url,title,domain) VALUES(1,'https://test.archive','Original title','test.archive')", [])?;
            let first = save_offline_archive(&root, conn, payload("original markdown"))?;
            let source = get_archive_base_dir(&root).join(&first.page_uuid); fs::create_dir_all(&source)?;
            fs::write(source.join("screenshot.png"), b"preserve original screenshot")?;
            conn.execute_batch("CREATE TRIGGER refuse_archive_update BEFORE UPDATE ON offline_archives BEGIN SELECT RAISE(ABORT,'synthetic archive failure'); END;")?;
            assert!(save_offline_archive(&root, conn, payload("must roll back index")).is_err());
            assert_eq!(conn.query_row("SELECT markdown FROM page_contents WHERE url_id=1", [], |r| r.get::<_,String>(0))?, "original markdown");
            assert_eq!(get_offline_archive(&root, conn, 1)?.unwrap().markdown_content.as_deref(), Some("original markdown"));
            conn.execute_batch("DROP TRIGGER refuse_archive_update;")?;
            let mut metadata = payload(""); metadata.markdown=None; metadata.html=None;
            let saved = save_offline_archive(&root, conn, metadata)?;
            assert_eq!(saved.id, first.id); assert!(saved.has_markdown && saved.has_snapshot_html);
            assert_eq!(get_offline_archive(&root, conn, 1)?.unwrap().markdown_content.as_deref(), Some("original markdown"));
            assert!(source.join("screenshot.png").exists());
            assert!(delete_offline_archive(&root, conn, "../outside").is_err());
            delete_offline_archive(&root, conn, &first.page_uuid)?;
            assert!(source.join("screenshot.png").exists());
            Ok(())
        }).unwrap();
        vault.lock().unwrap();
        drop(vault);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_and_expanding_html_are_rejected_with_output_bounds() {
        let valid = compress_html("<html>hello</html>").unwrap();
        assert_eq!(decompress_html(&valid).unwrap(), "<html>hello</html>");
        assert!(decompress_html(&[]).is_err());
        assert!(decompress_html(&valid[..valid.len() - 1]).is_err());
        let mut extra = valid;
        extra.extend_from_slice(b"extra");
        assert!(decompress_html(&extra).is_err());
        let large = "x".repeat(MAX_HTML_BYTES as usize + 1);
        assert!(compress_html(&large).is_err());
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(large.as_bytes()).unwrap();
        assert!(decompress_html(&encoder.finish().unwrap()).is_err());
    }

    #[test]
    fn test_offline_page_archive_lifecycle() {
        let temp_dir = std::env::temp_dir().join(format!("browsory_arch_{}", Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test.db");

        let mut conn = Connection::open(&db_path).unwrap();
        crate::database::migrations::run_migrations(&mut conn).unwrap();

        conn.execute(
            "INSERT INTO urls (id, url, title, domain) VALUES (1, 'https://example.com', 'Example', 'example.com');",
            [],
        ).unwrap();

        let payload = SaveArchivePayload {
            url_id: 1,
            archive_level: "full".into(),
            markdown: Some("# Header\n\nArticle body.".into()),
            html: Some("<html><body><h1>Header</h1><p>Article body.</p></body></html>".into()),
            title: Some("Example".into()),
            author: Some("Author".into()),
        };

        // 1. Save archive (stored encrypted in DB)
        let summary = save_offline_archive(&temp_dir, &conn, payload).unwrap();
        assert!(summary.has_markdown);
        assert!(summary.has_snapshot_html);

        // 2. Verify no plain files were left on disk
        let page_dir = get_archive_base_dir(&temp_dir).join(&summary.page_uuid);
        assert!(
            !page_dir.join("content.md").exists(),
            "Plain content.md must not exist on disk"
        );
        assert!(
            !page_dir.join("snapshot.html").exists(),
            "Plain snapshot.html must not exist on disk"
        );

        // 3. Read back archive
        let detail = get_offline_archive(&temp_dir, &conn, 1)
            .unwrap()
            .expect("Detail found");
        assert_eq!(
            detail.markdown_content.as_deref(),
            Some("# Header\n\nArticle body.")
        );
        assert!(detail.snapshot_html.unwrap().contains("<h1>Header</h1>"));

        // 4. Delete archive
        delete_offline_archive(&temp_dir, &conn, &summary.page_uuid).unwrap();
        let deleted = get_offline_archive(&temp_dir, &conn, 1).unwrap();
        assert!(deleted.is_none());

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
