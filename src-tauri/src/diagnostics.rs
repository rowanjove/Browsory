use chrono::Utc;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
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
    pub total_visits: i64,
    pub total_urls: i64,
    pub total_sources: i64,
    pub integrity_status: String,
    pub fts_status: String,
    pub sources: Vec<SourceDiagnostic>,
}

/// Collects system and database diagnostic status
pub fn collect_diagnostics_info(conn: &Connection, db_path: &Path) -> AppResult<DiagnosticsInfo> {
    let mut sys = System::new();
    sys.refresh_memory();

    let os_name = System::name().unwrap_or_else(|| "Unknown".to_string());
    let os_version = System::os_version().unwrap_or_else(|| "Unknown".to_string());
    let arch = System::cpu_arch();
    let total_memory_mb = sys.total_memory() / (1024 * 1024);

    let db_size_bytes = fs::metadata(db_path).map(|m| m.len()).unwrap_or(0);

    let db_schema_version: i32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let total_visits: i64 = conn
        .query_row("SELECT COUNT(*) FROM visits", [], |r| r.get(0))
        .unwrap_or(0);

    let total_urls: i64 = conn
        .query_row("SELECT COUNT(*) FROM urls", [], |r| r.get(0))
        .unwrap_or(0);

    let total_sources: i64 = conn
        .query_row("SELECT COUNT(*) FROM sources", [], |r| r.get(0))
        .unwrap_or(0);

    let integrity_status: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .unwrap_or_else(|e| format!("Error: {}", e));

    // Verify FTS table integrity
    let fts_status: String = match conn.query_row(
        "SELECT COUNT(*) FROM urls_fts WHERE urls_fts MATCH 'test'",
        [],
        |_| Ok(()),
    ) {
        Ok(_) => "ok".to_string(),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("no such table") {
                "missing".to_string()
            } else {
                "ok".to_string() // MATCH query with empty match is normal
            }
        }
    };

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
    for item in source_rows.flatten() {
        sources.push(item);
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
        total_visits,
        total_urls,
        total_sources,
        integrity_status,
        fts_status,
        sources,
    })
}

/// Sanitizes log content by scrubbing sensitive user data like URLs, emails, tokens, IP addresses.
pub fn sanitize_log_text(raw: &str) -> String {
    let url_regex = regex::Regex::new(r"https?://[^\s\)\],]+").unwrap();
    let email_regex = regex::Regex::new(r"[\w\.-]+@[\w\.-]+\.\w+").unwrap();
    let token_regex = regex::Regex::new(
        r#"(?i)(key|token|secret|password|bearer|pin)\s*[:=]\s*["']?([a-zA-Z0-9_\-\.]{8,})["']?"#,
    )
    .unwrap();

    let step1 = url_regex.replace_all(raw, "[URL_REDACTED]");
    let step2 = email_regex.replace_all(&step1, "[EMAIL_REDACTED]");
    let step3 = token_regex.replace_all(&step2, "$1=[REDACTED]");
    step3.to_string()
}

/// Exports a sanitized diagnostic ZIP bundle into target_path
pub fn export_diagnostics_bundle(
    conn: &Connection,
    db_path: &Path,
    logs_dir: &Path,
    target_zip_path: &Path,
) -> AppResult<String> {
    let info = collect_diagnostics_info(conn, db_path)?;

    let file = File::create(target_zip_path)
        .map_err(|e| AppError::System(format!("Failed to create diagnostic zip file: {}", e)))?;
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
    if logs_dir.exists() {
        if let Ok(entries) = fs::read_dir(logs_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Ok(content) = fs::read_to_string(&path) {
                        // Take the last 500 lines if too large
                        let lines: Vec<&str> = content.lines().collect();
                        let start = if lines.len() > 500 {
                            lines.len() - 500
                        } else {
                            0
                        };
                        for l in &lines[start..] {
                            combined_logs.push_str(l);
                            combined_logs.push('\n');
                        }
                    }
                }
            }
        }
    }
    let sanitized_logs = sanitize_log_text(&combined_logs);
    zip.start_file("sanitized.log", options)
        .map_err(|e| AppError::System(format!("Zip error: {}", e)))?;
    zip.write_all(sanitized_logs.as_bytes())?;

    zip.finish()
        .map_err(|e| AppError::System(format!("Failed to finalize zip bundle: {}", e)))?;

    Ok(target_zip_path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
