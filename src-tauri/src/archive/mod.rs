use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::error::AppResult;

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

pub fn save_offline_archive(
    app_dir: &Path,
    conn: &Connection,
    payload: SaveArchivePayload,
) -> AppResult<PageArchiveSummary> {
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
        .ok();

    let page_uuid = existing_uuid.unwrap_or_else(|| Uuid::new_v4().to_string());
    let page_dir = get_archive_base_dir(app_dir).join(&page_uuid);
    fs::create_dir_all(&page_dir)?;

    let mut size_bytes = 0u64;
    let has_markdown =
        payload.markdown.is_some() && !payload.markdown.as_ref().unwrap().trim().is_empty();
    let has_snapshot_html =
        payload.html.is_some() && !payload.html.as_ref().unwrap().trim().is_empty();

    // When replacing an archive with a lower fidelity payload, remove stale
    // content files so metadata flags cannot hide orphaned sensitive data.
    if !has_markdown {
        let md_path = page_dir.join("content.md");
        if md_path.exists() {
            fs::remove_file(md_path)?;
        }
    }
    if !has_snapshot_html {
        let html_path = page_dir.join("snapshot.html");
        if html_path.exists() {
            fs::remove_file(html_path)?;
        }
    }

    // 2. Save content.md if markdown provided
    if let Some(ref md) = payload.markdown.filter(|value| !value.trim().is_empty()) {
        let md_path = page_dir.join("content.md");
        fs::write(&md_path, md.as_bytes())?;
        size_bytes += md.len() as u64;

        // Also upsert into page_contents table for FTS5 full-text indexing
        let now = Utc::now().timestamp_millis();
        let word_count = md.split_whitespace().count() as i64;
        let author = payload.author.clone().unwrap_or_default();
        let _ = conn.execute(
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
        );
    }

    // 3. Save snapshot.html if html provided
    if let Some(ref html) = payload.html.filter(|value| !value.trim().is_empty()) {
        let html_path = page_dir.join("snapshot.html");
        fs::write(&html_path, html.as_bytes())?;
        size_bytes += html.len() as u64;
    }

    // 4. Save metadata.json
    let now = Utc::now().timestamp_millis();
    let relative_path = format!("pages/{}", page_uuid);

    let metadata_json = serde_json::json!({
        "page_uuid": page_uuid,
        "url_id": payload.url_id,
        "url": url,
        "title": title,
        "domain": domain,
        "archive_level": payload.archive_level,
        "relative_path": relative_path,
        "has_markdown": has_markdown,
        "has_snapshot_html": has_snapshot_html,
        "has_screenshot": false,
        "size_bytes": size_bytes,
        "archived_at": now,
    });
    let meta_str = serde_json::to_string_pretty(&metadata_json).unwrap_or_default();
    fs::write(page_dir.join("metadata.json"), meta_str.as_bytes())?;
    size_bytes += meta_str.len() as u64;

    // 5. Upsert into offline_archives table
    conn.execute(
        r#"
        INSERT INTO offline_archives (
            url_id, page_uuid, archive_level, relative_path, title, domain,
            has_markdown, has_snapshot_html, has_screenshot, size_bytes, archived_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9, ?10)
        ON CONFLICT(page_uuid) DO UPDATE SET
            archive_level = excluded.archive_level,
            has_markdown = excluded.has_markdown,
            has_snapshot_html = excluded.has_snapshot_html,
            size_bytes = excluded.size_bytes,
            archived_at = excluded.archived_at
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
            now
        ],
    )?;

    let record_id = conn.last_insert_rowid();

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
        has_screenshot: false,
        size_bytes,
        archived_at: now,
    })
}

pub fn get_offline_archive(
    app_dir: &Path,
    conn: &Connection,
    url_id: i64,
) -> AppResult<Option<PageArchiveDetail>> {
    let summary: Option<PageArchiveSummary> = conn
        .query_row(
            r#"
            SELECT oa.id, oa.url_id, oa.page_uuid, u.url, oa.title, oa.domain,
                   oa.archive_level, oa.relative_path, oa.has_markdown,
                   oa.has_snapshot_html, oa.has_screenshot, oa.size_bytes, oa.archived_at
            FROM offline_archives oa
            JOIN urls u ON oa.url_id = u.id
            WHERE oa.url_id = ?1
            "#,
            params![url_id],
            |r| {
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
            },
        )
        .ok();

    match summary {
        Some(s) => {
            let page_dir = get_archive_base_dir(app_dir).join(&s.page_uuid);
            let markdown_content = if s.has_markdown {
                fs::read_to_string(page_dir.join("content.md")).ok()
            } else {
                None
            };
            let snapshot_html = if s.has_snapshot_html {
                fs::read_to_string(page_dir.join("snapshot.html")).ok()
            } else {
                None
            };

            Ok(Some(PageArchiveDetail {
                summary: s,
                markdown_content,
                snapshot_html,
            }))
        }
        None => Ok(None),
    }
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

    Ok(rows.flatten().collect())
}

pub fn delete_offline_archive(app_dir: &Path, conn: &Connection, page_uuid: &str) -> AppResult<()> {
    let parsed_uuid = Uuid::parse_str(page_uuid)
        .map_err(|_| crate::error::AppError::Parse("无效的归档 page_uuid".into()))?;
    let canonical_uuid = parsed_uuid.to_string();
    if canonical_uuid != page_uuid {
        return Err(crate::error::AppError::Parse(
            "归档 page_uuid 必须是标准 UUID 格式".into(),
        ));
    }

    let archive_base = get_archive_base_dir(app_dir);
    let page_dir = archive_base.join(&canonical_uuid);
    if page_dir.exists() {
        fs::remove_dir_all(&page_dir)?;
    }
    conn.execute(
        "DELETE FROM offline_archives WHERE page_uuid = ?1",
        params![canonical_uuid],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::migrations::run_migrations;

    #[test]
    fn test_offline_page_archive_lifecycle() {
        let temp_dir = std::env::temp_dir().join(format!(
            "browsory_test_archive_{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let _ = fs::create_dir_all(&temp_dir);
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();

        conn.execute(
            "INSERT INTO urls (id, url, title, domain) VALUES (1, 'https://example.com/rust-post', 'Rust Async Cancellation', 'example.com')",
            [],
        ).unwrap();

        let payload = SaveArchivePayload {
            url_id: 1,
            archive_level: "snapshot".to_string(),
            markdown: Some("# Rust Async\nDeep dive into async cancellation tokens.".to_string()),
            html: Some("<html><body><h1>Rust Async</h1></body></html>".to_string()),
            title: Some("Rust Async Cancellation".to_string()),
            author: Some("Ferris".to_string()),
        };

        // 1. Save archive
        let summary = save_offline_archive(&temp_dir, &conn, payload).unwrap();
        assert_eq!(summary.url_id, 1);
        assert!(summary.has_markdown);
        assert!(summary.has_snapshot_html);

        // 2. Fetch archive
        let detail = get_offline_archive(&temp_dir, &conn, 1).unwrap().unwrap();
        assert_eq!(detail.summary.page_uuid, summary.page_uuid);
        assert!(detail
            .markdown_content
            .unwrap()
            .contains("cancellation tokens"));
        assert!(detail
            .snapshot_html
            .unwrap()
            .contains("<h1>Rust Async</h1>"));

        // 3. Verify FTS table page_contents_fts also has the record
        let fts_title: String = conn
            .query_row(
                "SELECT title FROM page_contents_fts WHERE page_contents_fts MATCH 'Async'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fts_title, "Rust Async Cancellation");

        // 4. Delete archive
        delete_offline_archive(&temp_dir, &conn, &summary.page_uuid).unwrap();
        let after_delete = get_offline_archive(&temp_dir, &conn, 1).unwrap();
        assert!(after_delete.is_none());

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_delete_rejects_path_traversal_and_noncanonical_uuid() {
        let temp_dir = std::env::temp_dir().join(format!(
            "browsory_test_archive_path_{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        fs::create_dir_all(&temp_dir).unwrap();
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();

        let outside = temp_dir.join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("keep.txt"), b"keep").unwrap();

        assert!(delete_offline_archive(&temp_dir, &conn, "../outside").is_err());
        assert!(outside.join("keep.txt").exists());
        assert!(
            delete_offline_archive(&temp_dir, &conn, "550E8400-E29B-41D4-A716-446655440000")
                .is_err()
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
