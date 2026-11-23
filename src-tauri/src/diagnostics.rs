use chrono::Utc;
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::OnceLock;
use sysinfo::System;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceDiagnostic {
    pub id: i64,
    pub browser: String,
    pub profile: String,
    pub source_type: String,
    pub enabled: bool,
    pub last_visit_time: i64,
    pub last_sync_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticsInfo {
    pub app_version: String,
    pub os_name: String,
    pub os_version: String,
    pub arch: String,
    pub total_memory_mb: u64,
    pub db_path: String,
    pub db_size_bytes: u64,
    pub db_schema_version: i32,
    #[serde(default)]
    pub sqlite_version: String,
    #[serde(default)]
    pub sqlcipher_version: Option<String>,
    pub total_visits: i64,
    pub total_urls: i64,
    pub total_sources: i64,
    pub integrity_status: String,
    pub fts_status: String,
    pub sources: Vec<SourceDiagnostic>,
}

/// Collects system and database diagnostic status using full integrity verification.
pub fn collect_diagnostics_info(conn: &Connection, db_path: &Path) -> AppResult<DiagnosticsInfo> {
    collect_diagnostics_info_internal(conn, db_path, true)
}

/// Collects system and database diagnostic status using fast read-only checks.
pub fn collect_diagnostics_info_fast(
    conn: &Connection,
    db_path: &Path,
) -> AppResult<DiagnosticsInfo> {
    collect_diagnostics_info_internal(conn, db_path, false)
}

fn collect_diagnostics_info_internal(
    conn: &Connection,
    db_path: &Path,
    deep: bool,
) -> AppResult<DiagnosticsInfo> {
    let mut sys = System::new();
    sys.refresh_memory();

    let os_name = System::name().unwrap_or_else(|| "Unknown".to_string());
    let os_version = System::os_version().unwrap_or_else(|| "Unknown".to_string());
    let arch = System::cpu_arch();
    let total_memory_mb = sys.total_memory() / (1024 * 1024);

    let db_size_bytes = fs::metadata(db_path).map(|m| m.len()).unwrap_or(0);

    let db_schema_version: i32 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |r| r.get(0),
    )?;
    let sqlite_version: String = conn.query_row("SELECT sqlite_version()", [], |r| r.get(0))?;
    let sqlcipher_version: Option<String> = conn
        .query_row("PRAGMA cipher_version", [], |r| r.get(0))
        .optional()?;

    let total_visits: i64 = conn.query_row("SELECT COUNT(*) FROM visits", [], |r| r.get(0))?;

    let total_urls: i64 = conn.query_row("SELECT COUNT(*) FROM urls", [], |r| r.get(0))?;

    let total_sources: i64 = conn.query_row("SELECT COUNT(*) FROM sources", [], |r| r.get(0))?;

    let integrity_status: String = if deep {
        conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?
    } else {
        conn.query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?
    };

    // Deep check verifies internal FTS5 postings and vocab mappings; fast check relies on
    // indexed FTS5 shadow docsize IDs so UI refreshes remain non-blocking and read-only.
    let fts_status = check_fts_status_with_mode(conn, total_urls, deep)?;

    // Sources diagnostics (strictly technical IDs, no user browsing data)
    let mut stmt = conn.prepare(
        "SELECT id, browser, profile, source_type, enabled, last_visit_time, last_sync_at FROM sources",
    )?;
    let source_rows = stmt.query_map([], |r| {
        Ok(SourceDiagnostic {
            id: r.get(0)?,
            browser: r.get(1)?,
            profile: r.get(2)?,
            source_type: r.get(3)?,
            enabled: r.get::<_, i64>(4)? == 1,
            last_visit_time: r.get(5)?,
            last_sync_at: r.get(6)?,
        })
    })?;

    let mut sources = Vec::new();
    for item in source_rows {
        sources.push(item?);
    }

    Ok(DiagnosticsInfo {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        os_name,
        os_version,
        arch,
        total_memory_mb,
        db_path: db_path.to_string_lossy().to_string(),
        db_size_bytes,
        db_schema_version,
        sqlite_version,
        sqlcipher_version,
        total_visits,
        total_urls,
        total_sources,
        integrity_status,
        fts_status,
        sources,
    })
}

#[cfg_attr(not(test), allow(dead_code))]
fn check_fts_status(conn: &Connection, total_urls: i64) -> AppResult<String> {
    check_fts_status_with_mode(conn, total_urls, true)
}

fn check_fts_status_with_mode(conn: &Connection, total_urls: i64, deep: bool) -> AppResult<String> {
    if deep {
        conn.execute(
            "INSERT INTO urls_fts(urls_fts) VALUES ('integrity-check')",
            [],
        )?;
    }

    // An external-content FTS5 table still maintains one row per indexed document in its
    // shadow `%_docsize` table with an INTEGER PRIMARY KEY matching `urls.id`. Querying it
    // verifies document coverage on O(urls) primary keys without scanning every token posting
    // and remains compatible with `PRAGMA query_only = true` when `deep` is false.
    let indexed_rows: i64 = conn.query_row("SELECT COUNT(*) FROM urls_fts_docsize", [], |row| {
        row.get(0)
    })?;
    let missing_rows: i64 = conn.query_row(
        "SELECT COUNT(*) FROM urls u WHERE NOT EXISTS (SELECT 1 FROM urls_fts_docsize f WHERE f.id = u.id)",
        [],
        |row| row.get(0),
    )?;
    let orphan_rows: i64 = conn.query_row(
        "SELECT COUNT(*) FROM urls_fts_docsize f WHERE NOT EXISTS (SELECT 1 FROM urls u WHERE u.id = f.id)",
        [],
        |row| row.get(0),
    )?;

    if indexed_rows == total_urls && missing_rows == 0 && orphan_rows == 0 {
        Ok("ok".to_string())
    } else {
        Ok(format!(
            "inconsistent: fts_rows={}, urls={}, missing={}, orphan={}",
            indexed_rows, total_urls, missing_rows, orphan_rows
        ))
    }
}

/// Sanitizes log content by scrubbing sensitive user data like URLs, emails, tokens, IP addresses.
pub fn sanitize_log_text(raw: &str) -> String {
    static URL_REGEX: OnceLock<regex::Regex> = OnceLock::new();
    static EMAIL_REGEX: OnceLock<regex::Regex> = OnceLock::new();
    static TOKEN_REGEX: OnceLock<regex::Regex> = OnceLock::new();

    let url_regex = URL_REGEX.get_or_init(|| regex::Regex::new(r"https?://[^\s\)\],]+").unwrap());
    let email_regex =
        EMAIL_REGEX.get_or_init(|| regex::Regex::new(r"[\w\.-]+@[\w\.-]+\.\w+").unwrap());
    let token_regex = TOKEN_REGEX.get_or_init(|| {
        regex::Regex::new(
            r#"(?i)(key|token|secret|password|bearer|pin)\s*[:=]\s*["']?([a-zA-Z0-9_\-\.]{8,})["']?"#,
        )
        .unwrap()
    });

    let step1 = url_regex.replace_all(raw, "[URL_REDACTED]");
    let step2 = email_regex.replace_all(&step1, "[EMAIL_REDACTED]");
    let step3 = token_regex.replace_all(&step2, "$1=[REDACTED]");
    step3.to_string()
}

const MAX_DIAG_LOG_TAIL_BYTES: u64 = 2 * 1024 * 1024;

fn read_bounded_log_tail(path: &Path) -> Option<String> {
    let meta = crate::security::fs_boundary::plain_metadata(path).ok()??;
    if !meta.is_file() {
        return None;
    }
    let mut file = File::open(path).ok()?;
    let len = meta.len();
    let seeked = len > MAX_DIAG_LOG_TAIL_BYTES;
    if seeked {
        file.seek(SeekFrom::End(-(MAX_DIAG_LOG_TAIL_BYTES as i64)))
            .ok()?;
    }
    let mut bytes = Vec::with_capacity(len.min(MAX_DIAG_LOG_TAIL_BYTES) as usize);
    file.take(MAX_DIAG_LOG_TAIL_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if seeked {
        // Drop potentially cut first UTF-8 line after seeking from end.
        if let Some((_, rest)) = text.split_once('\n') {
            return Some(rest.to_string());
        }
    }
    Some(text)
}

/// Exports a sanitized diagnostic ZIP bundle into target_path.
#[cfg_attr(not(test), allow(dead_code))]
pub fn export_diagnostics_bundle(
    conn: &Connection,
    db_path: &Path,
    logs_dir: &Path,
    target_zip_path: &Path,
) -> AppResult<String> {
    let info = collect_diagnostics_info(conn, db_path)?;
    export_diagnostics_bundle_with_info(&info, logs_dir, target_zip_path)
}

/// Exports a sanitized diagnostic ZIP bundle using pre-collected diagnostics info.
pub fn export_diagnostics_bundle_with_info(
    info: &DiagnosticsInfo,
    logs_dir: &Path,
    target_zip_path: &Path,
) -> AppResult<String> {
    let parent = target_zip_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let file_name = target_zip_path
        .file_name()
        .ok_or_else(|| AppError::Parse("诊断包文件名不能为空".into()))?
        .to_string_lossy();
    let partial_path = parent.join(format!(
        ".{}.browsory-diag-{}.partial",
        file_name,
        uuid::Uuid::new_v4()
    ));

    let write_res: AppResult<()> = (|| {
        let file = File::create(&partial_path).map_err(|e| {
            AppError::System(format!("Failed to create diagnostic zip file: {}", e))
        })?;
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o644);

        // 1. system.json
        let system_json = serde_json::to_string_pretty(&serde_json::json!({
            "app_version": info.app_version,
            "os_name": info.os_name,
            "os_version": info.os_version,
            "arch": info.arch,
            "total_memory_mb": info.total_memory_mb,
            "generated_at": Utc::now().to_rfc3339()
        }))?;
        zip.start_file("system.json", options)
            .map_err(|e| AppError::System(format!("Zip error: {}", e)))?;
        zip.write_all(system_json.as_bytes())?;

        // 2. database.json (strictly technical stats, no URL or title contents)
        let database_json = serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": info.db_schema_version,
            "sqlite_version": info.sqlite_version,
            "sqlcipher_version": info.sqlcipher_version,
            "size_bytes": info.db_size_bytes,
            "total_visits": info.total_visits,
            "total_urls": info.total_urls,
            "total_sources": info.total_sources,
            "integrity_status": info.integrity_status,
            "fts_status": info.fts_status
        }))?;
        zip.start_file("database.json", options)
            .map_err(|e| AppError::System(format!("Zip error: {}", e)))?;
        zip.write_all(database_json.as_bytes())?;

        // 3. sources.json
        let sources_json = serde_json::to_string_pretty(&info.sources)?;
        zip.start_file("sources.json", options)
            .map_err(|e| AppError::System(format!("Zip error: {}", e)))?;
        zip.write_all(sources_json.as_bytes())?;

        // 4. sanitized.log
        let mut combined_logs = String::new();
        if crate::security::fs_boundary::plain_metadata(logs_dir)
            .ok()
            .flatten()
            .is_some_and(|meta| meta.is_dir())
        {
            if let Ok(entries) = fs::read_dir(logs_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Some(content) = read_bounded_log_tail(&path) {
                        // Take the last 500 lines if too large
                        let lines: Vec<&str> = content.lines().collect();
                        let start = lines.len().saturating_sub(500);
                        for l in &lines[start..] {
                            combined_logs.push_str(l);
                            combined_logs.push('\n');
                        }
                    }
                }
            }
        }
        let sanitized_logs = sanitize_log_text(&combined_logs);
        zip.start_file("sanitized.log", options)
            .map_err(|e| AppError::System(format!("Zip error: {}", e)))?;
        zip.write_all(sanitized_logs.as_bytes())?;

        let finished_file = zip
            .finish()
            .map_err(|e| AppError::System(format!("Failed to finalize zip bundle: {}", e)))?;
        finished_file.sync_all()?;
        drop(finished_file);

        fs::rename(&partial_path, target_zip_path)?;
        Ok(())
    })();

    if write_res.is_err() {
        let _ = fs::remove_file(&partial_path);
    }
    write_res?;

    Ok(target_zip_path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fts_integrity_and_content_mismatch_are_reported() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE urls(id INTEGER PRIMARY KEY, title TEXT, url TEXT, domain TEXT);
             CREATE VIRTUAL TABLE urls_fts USING fts5(title, url, domain, content='urls', content_rowid='id');
             INSERT INTO urls(id, title, url, domain) VALUES(1, 'title', 'https://example.test', 'example.test');",
        )
        .unwrap();

        let status = check_fts_status(&conn, 1).unwrap();
        assert!(status.starts_with("inconsistent:"));

        conn.execute(
            "INSERT INTO urls_fts(rowid, title, url, domain) VALUES(1, 'title', 'https://example.test', 'example.test')",
            [],
        )
        .unwrap();
        assert_eq!(check_fts_status(&conn, 1).unwrap(), "ok");
    }

    #[test]
    fn fts_errors_propagate_instead_of_being_reported_as_ok() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute("CREATE TABLE urls(id INTEGER PRIMARY KEY)", [])
            .unwrap();
        let error = check_fts_status(&conn, 0).unwrap_err().to_string();
        assert!(error.contains("urls_fts"));
    }

    #[test]
    fn test_sanitize_log_text_strips_urls_emails_and_keys() {
        let input = "User visited https://bank.com/account?secret=123 with email test.user@company.com and apiKey: sk-abcdef1234567890 in session.";
        let sanitized = sanitize_log_text(input);

        assert!(!sanitized.contains("https://bank.com"));
        assert!(sanitized.contains("[URL_REDACTED]"));

        assert!(!sanitized.contains("test.user@company.com"));
        assert!(sanitized.contains("[EMAIL_REDACTED]"));

        assert!(!sanitized.contains("sk-abcdef1234567890"));
        assert!(sanitized.contains("[REDACTED]"));
    }

    #[test]
    fn collect_diagnostics_info_fast_works_on_query_only_connection() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY);
             INSERT INTO schema_migrations(version) VALUES (1);
             CREATE TABLE sources(
                 id INTEGER PRIMARY KEY,
                 browser TEXT NOT NULL,
                 profile TEXT NOT NULL,
                 source_type TEXT NOT NULL,
                 enabled INTEGER NOT NULL,
                 last_visit_time INTEGER,
                 last_sync_at INTEGER
             );
             CREATE TABLE urls(id INTEGER PRIMARY KEY, title TEXT, url TEXT, domain TEXT);
             CREATE TABLE visits(id INTEGER PRIMARY KEY, url_id INTEGER);
             CREATE VIRTUAL TABLE urls_fts USING fts5(title, url, domain, content='urls', content_rowid='id');
             INSERT INTO urls(id, title, url, domain) VALUES(1, 'title', 'https://example.test', 'example.test');",
        )
        .unwrap();

        conn.pragma_update(None, "query_only", true).unwrap();
        let mismatch = collect_diagnostics_info_fast(&conn, Path::new("nonexistent.db")).unwrap();
        assert_eq!(mismatch.integrity_status, "ok");
        assert!(mismatch.fts_status.starts_with("inconsistent:"));

        conn.pragma_update(None, "query_only", false).unwrap();
        conn.execute(
            "INSERT INTO urls_fts(rowid, title, url, domain) VALUES(1, 'title', 'https://example.test', 'example.test')",
            [],
        )
        .unwrap();
        conn.pragma_update(None, "query_only", true).unwrap();

        let healthy = collect_diagnostics_info_fast(&conn, Path::new("nonexistent.db")).unwrap();
        assert_eq!(healthy.integrity_status, "ok");
        assert_eq!(healthy.fts_status, "ok");
        assert_eq!(healthy.total_urls, 1);
    }
}
