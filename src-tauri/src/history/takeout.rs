use chrono::Utc;
use rusqlite::{params, Connection};
use serde::Deserialize;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::time::Instant;
use tracing::info;

use crate::database::models::TakeoutImportSummary;
use crate::error::{AppError, AppResult};
use crate::history::dedup::calculate_event_hash;
use crate::history::normalize::normalize_url;

#[derive(Debug, Deserialize)]
struct TakeoutRecordItem {
    pub url: Option<String>,
    pub title: Option<String>,
    #[serde(default)]
    pub time_usec: i64,
    #[serde(default)]
    pub timestamp_usec: i64,
}

#[derive(Debug, Deserialize)]
struct TakeoutWrapper {
    #[serde(rename = "Browser History")]
    pub browser_history: Option<Vec<TakeoutRecordItem>>,
    pub records: Option<Vec<TakeoutRecordItem>>,
}

pub fn import_google_takeout_file(
    conn: &mut Connection,
    file_path: &Path,
) -> AppResult<TakeoutImportSummary> {
    let start_time = Instant::now();
    let now = Utc::now().timestamp_millis();

    let file = File::open(file_path).map_err(|e| {
        AppError::Other(format!(
            "无法打开 Google Takeout 文件 {:?}: {}",
            file_path, e
        ))
    })?;
    let reader = BufReader::new(file);

    // Try parsing as wrapper { "Browser History": [...] } or direct array [...]
    let raw_items: Vec<TakeoutRecordItem> =
        match serde_json::from_reader::<_, TakeoutWrapper>(reader) {
            Ok(wrapper) => wrapper
                .browser_history
                .or(wrapper.records)
                .unwrap_or_default(),
            Err(_) => {
                // Re-open and try as plain array
                let file2 = File::open(file_path)?;
                let reader2 = BufReader::new(file2);
                serde_json::from_reader(reader2)
                    .map_err(|e| AppError::Other(format!("Google Takeout JSON 解析失败: {}", e)))?
            }
        };

    let total_parsed = raw_items.len() as u64;

    // Ensure a source exists for Google Takeout
    let source_id: i64 = conn
        .query_row(
            "SELECT id FROM sources WHERE browser = 'takeout' AND profile = 'Default'",
            [],
            |r| r.get(0),
        )
        .or_else(|_| {
            conn.execute(
                r#"
                INSERT INTO sources (browser, profile, source_type, history_path, enabled, last_visit_id, last_visit_time, last_sync_at)
                VALUES ('takeout', 'Default', 'takeout', ?1, 1, 0, 0, ?2)
                "#,
                params![file_path.to_string_lossy(), now],
            )?;
            Ok::<i64, rusqlite::Error>(conn.last_insert_rowid())
        })?;

    let tx = conn.transaction()?;

    let mut urls_inserted = 0u64;
    let mut visits_inserted = 0u64;
    let mut duplicates_skipped = 0u64;

    {
        let mut insert_url_stmt = tx.prepare_cached(
            r#"
            INSERT INTO urls (url, title, domain, normalized_url)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(url) DO UPDATE SET
                title = CASE WHEN length(?2) > 0 THEN ?2 ELSE title END
            RETURNING id;
            "#,
        )?;

        let mut insert_visit_stmt = tx.prepare_cached(
            r#"
            INSERT OR IGNORE INTO visits (
                source_id, url_id, source_visit_id, visit_time,
                transition, from_visit, visit_duration, event_hash, imported_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
        )?;

        for (idx, item) in raw_items.into_iter().enumerate() {
            let url_str = match item.url.as_deref() {
                Some(u) if !u.trim().is_empty() => u.trim().to_string(),
                _ => continue,
            };

            if url_str.starts_with("javascript:")
                || url_str.starts_with("data:")
                || url_str == "about:blank"
            {
                continue;
            }

            let title_str = item.title.unwrap_or_default();
            let norm = normalize_url(&url_str);

            let micros = if item.time_usec > 0 {
                item.time_usec
            } else {
                item.timestamp_usec
            };
            let visit_time_ms = if micros > 0 { micros / 1000 } else { now };

            let url_id: i64 = insert_url_stmt.query_row(
                params![url_str, title_str, norm.domain, norm.normalized_url],
                |r| r.get(0),
            )?;
            urls_inserted += 1;

            let event_hash =
                calculate_event_hash(source_id, idx as i64, visit_time_ms, &norm.normalized_url);
            let rows_affected = insert_visit_stmt.execute(params![
                source_id,
                url_id,
                idx as i64,
                visit_time_ms,
                0,
                0,
                0,
                event_hash,
                now
            ])?;

            if rows_affected > 0 {
                visits_inserted += 1;
            } else {
                duplicates_skipped += 1;
            }
        }
    }

    tx.commit()?;

    let duration_ms = start_time.elapsed().as_millis() as u64;
    info!(
        "Takeout import complete: parsed {}, visits inserted {}, dupes {}, duration {}ms",
        total_parsed, visits_inserted, duplicates_skipped, duration_ms
    );

    Ok(TakeoutImportSummary {
        total_records_parsed: total_parsed,
        urls_inserted,
        visits_inserted,
        duplicates_skipped,
        duration_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_import_takeout_flow() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::database::migrations::run_migrations(&mut conn).unwrap();

        // Create temporary test JSON
        let temp_dir = std::env::temp_dir().join("browsory_test_takeout");
        let _ = std::fs::create_dir_all(&temp_dir);
        let sample_file = temp_dir.join("BrowserHistory.json");

        let sample_json = r#"{
            "Browser History": [
                {
                    "url": "https://rust-lang.org",
                    "title": "Rust Programming Language",
                    "time_usec": 1700000000000000
                },
                {
                    "url": "https://tauri.app",
                    "title": "Tauri Framework",
                    "time_usec": 1700000001000000
                }
            ]
        }"#;
        let mut f = std::fs::File::create(&sample_file).unwrap();
        f.write_all(sample_json.as_bytes()).unwrap();
        drop(f);

        let summary =
            import_google_takeout_file(&mut conn, &sample_file).expect("Takeout import failed");
        assert_eq!(summary.total_records_parsed, 2);
        assert_eq!(summary.urls_inserted, 2);
        assert_eq!(summary.visits_inserted, 2);

        // Verify that source was created with correct schema
        let source_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM sources WHERE browser = 'takeout'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(source_count, 1);

        // Clean up
        let _ = std::fs::remove_file(&sample_file);
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
