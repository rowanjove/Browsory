use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
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

#[derive(Debug, Clone, Deserialize)]
struct TakeoutRecordItem {
    pub url: Option<String>,
    #[serde(rename = "titleUrl")]
    pub title_url: Option<String>,
    pub virtual_url: Option<String>,
    pub title: Option<String>,
    pub header: Option<String>,
    #[serde(default)]
    pub time_usec: Option<serde_json::Value>,
    #[serde(default)]
    pub timestamp_usec: Option<serde_json::Value>,
    #[serde(default)]
    pub timestamp_msec: Option<serde_json::Value>,
    #[serde(default)]
    pub time: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TakeoutWrapper {
    #[serde(rename = "Browser History")]
    pub browser_history: Option<Vec<TakeoutRecordItem>>,
    #[serde(rename = "浏览器历史记录")]
    pub browser_history_zh: Option<Vec<TakeoutRecordItem>>,
    #[serde(rename = "历史记录")]
    pub history_zh: Option<Vec<TakeoutRecordItem>>,
    #[serde(rename = "瀏覽器歷史記錄")]
    pub browser_history_zht: Option<Vec<TakeoutRecordItem>>,
    #[serde(rename = "歷史記錄")]
    pub history_zht: Option<Vec<TakeoutRecordItem>>,
    pub records: Option<Vec<TakeoutRecordItem>>,
    #[serde(rename = "Records")]
    pub records_capital: Option<Vec<TakeoutRecordItem>>,
    pub history: Option<Vec<TakeoutRecordItem>>,
    #[serde(rename = "BrowserHistory")]
    pub browser_history_camel: Option<Vec<TakeoutRecordItem>>,
}

fn parse_json_raw_micros(val: &Option<serde_json::Value>) -> Option<i64> {
    if let Some(v) = val {
        if let Some(num) = v.as_i64() {
            return Some(num);
        }
        if let Some(num) = v.as_u64() {
            return Some(num as i64);
        }
        if let Some(s) = v.as_str() {
            if let Ok(num) = s.trim().parse::<i64>() {
                return Some(num);
            }
        }
    }
    None
}

fn parse_json_timestamp(val: &Option<serde_json::Value>, is_msec: bool) -> Option<i64> {
    if let Some(v) = val {
        if let Some(num) = v.as_i64() {
            return Some(if is_msec { num } else { num / 1000 });
        }
        if let Some(num) = v.as_u64() {
            return Some(if is_msec {
                num as i64
            } else {
                (num as i64) / 1000
            });
        }
        if let Some(s) = v.as_str() {
            if let Ok(num) = s.trim().parse::<i64>() {
                return Some(if is_msec { num } else { num / 1000 });
            }
        }
    }
    None
}

fn parse_iso_time(time_str: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(time_str)
        .or_else(|_| chrono::DateTime::parse_from_str(time_str, "%Y-%m-%d %H:%M:%S %z"))
        .map(|dt| dt.timestamp_millis())
        .ok()
}

fn parse_takeout_items(file_path: &Path) -> AppResult<Vec<TakeoutRecordItem>> {
    let file = File::open(file_path).map_err(|e| {
        AppError::Other(format!(
            "无法打开 Google Takeout 文件 {:?}: {}",
            file_path, e
        ))
    })?;
    let reader = BufReader::new(file);

    // 1. Try known wrapper keys (English, Chinese Simplified, Chinese Traditional, records)
    if let Ok(wrapper) = serde_json::from_reader::<_, TakeoutWrapper>(reader) {
        let items_opt = wrapper
            .browser_history
            .or(wrapper.browser_history_zh)
            .or(wrapper.history_zh)
            .or(wrapper.browser_history_zht)
            .or(wrapper.history_zht)
            .or(wrapper.records)
            .or(wrapper.records_capital)
            .or(wrapper.history)
            .or(wrapper.browser_history_camel);
        if let Some(items) = items_opt {
            if !items.is_empty() {
                return Ok(items);
            }
        }
    }

    // 2. Re-open and try as plain array [...]
    let file2 = File::open(file_path)?;
    let reader2 = BufReader::new(file2);
    if let Ok(items) = serde_json::from_reader::<_, Vec<TakeoutRecordItem>>(reader2) {
        if !items.is_empty() {
            return Ok(items);
        }
    }

    // 3. Fallback: Parse as Value and find any array with record-like objects
    let file3 = File::open(file_path)?;
    let reader3 = BufReader::new(file3);
    let val: serde_json::Value = serde_json::from_reader(reader3)
        .map_err(|e| AppError::Other(format!("Google Takeout JSON 解析失败: {}", e)))?;

    if let serde_json::Value::Object(map) = val {
        for (_k, v) in map {
            if let serde_json::Value::Array(_) = v {
                if let Ok(items) = serde_json::from_value::<Vec<TakeoutRecordItem>>(v) {
                    if !items.is_empty() {
                        return Ok(items);
                    }
                }
            }
        }
    }

    Err(AppError::Other(format!(
        "未能从 {:?} 中解析出历史记录数据。请确认该文件为 Google Takeout 导出的 Chrome 历史记录或活动记录 JSON 归档",
        file_path.file_name().unwrap_or_default()
    )))
}

pub fn import_google_takeout_file(
    conn: &mut Connection,
    file_path: &Path,
) -> AppResult<TakeoutImportSummary> {
    let start_time = Instant::now();
    let now = Utc::now().timestamp_millis();

    let raw_items = parse_takeout_items(file_path)?;
    let total_parsed = raw_items.len() as u64;

    // Ensure a source exists for Google Takeout
    let source_id: i64 = match conn
        .query_row(
            "SELECT id FROM sources WHERE browser = 'takeout' AND profile = 'Default'",
            [],
            |r| r.get(0),
        )
        .optional()?
    {
        Some(id) => id,
        None => {
            conn.execute(
                r#"
                INSERT INTO sources (browser, profile, source_type, history_path, enabled, last_visit_id, last_visit_time, last_sync_at)
                VALUES ('takeout', 'Default', 'takeout', ?1, 1, 0, 0, ?2)
                "#,
                params![file_path.to_string_lossy(), now],
            )?;
            conn.last_insert_rowid()
        }
    };

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

        for item in raw_items {
            let url_opt = item
                .url
                .or(item.title_url)
                .or(item.virtual_url)
                .map(|u| u.trim().to_string());

            let url_str = match url_opt {
                Some(u) if !u.is_empty() => u,
                _ => continue,
            };

            if url_str.starts_with("javascript:")
                || url_str.starts_with("data:")
                || url_str == "about:blank"
            {
                continue;
            }

            let title_str = item.title.or(item.header).unwrap_or_default();
            let norm = normalize_url(&url_str);

            let raw_usec = parse_json_raw_micros(&item.time_usec)
                .or_else(|| parse_json_raw_micros(&item.timestamp_usec))
                .or_else(|| parse_json_timestamp(&item.timestamp_msec, true).map(|ms| ms * 1000))
                .or_else(|| {
                    item.time
                        .as_deref()
                        .and_then(parse_iso_time)
                        .map(|ms| ms * 1000)
                })
                .unwrap_or(now * 1000);

            let visit_time_ms = raw_usec / 1000;
            let stable_source_visit_id = raw_usec;

            let url_id: i64 = insert_url_stmt.query_row(
                params![url_str, title_str, norm.domain, norm.normalized_url],
                |r| r.get(0),
            )?;
            urls_inserted += 1;

            let event_hash = calculate_event_hash(
                source_id,
                stable_source_visit_id,
                visit_time_ms,
                &norm.normalized_url,
            );
            let rows_affected = insert_visit_stmt.execute(params![
                source_id,
                url_id,
                stable_source_visit_id,
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

    #[test]
    fn test_import_takeout_chinese_and_extended_formats() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::database::migrations::run_migrations(&mut conn).unwrap();

        let temp_dir = std::env::temp_dir().join("browsory_test_takeout_zh");
        let _ = std::fs::create_dir_all(&temp_dir);
        let sample_file = temp_dir.join("历史记录.json");

        let sample_json = r#"{
            "历史记录": [
                {
                    "titleUrl": "https://bing.com",
                    "title": "必应搜索",
                    "time_usec": "1700000002000000"
                },
                {
                    "url": "https://github.com",
                    "header": "GitHub",
                    "time": "2024-01-01T12:00:00.000Z"
                }
            ]
        }"#;
        let mut f = std::fs::File::create(&sample_file).unwrap();
        f.write_all(sample_json.as_bytes()).unwrap();
        drop(f);

        let summary = import_google_takeout_file(&mut conn, &sample_file)
            .expect("Takeout Chinese format import failed");
        assert_eq!(summary.total_records_parsed, 2);
        assert_eq!(summary.visits_inserted, 2);

        // Re-importing the same records must result in 100% duplicate skips (idempotent)
        let summary2 = import_google_takeout_file(&mut conn, &sample_file)
            .expect("Takeout second import failed");
        assert_eq!(summary2.total_records_parsed, 2);
        assert_eq!(summary2.visits_inserted, 0);
        assert_eq!(summary2.duplicates_skipped, 2);

        let _ = std::fs::remove_file(&sample_file);
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
