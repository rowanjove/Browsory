use rusqlite::{params, Connection, OpenFlags};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::info;

use super::{BrowserAdapter, DiscoveredProfile};
use crate::error::AppResult;
use crate::history::parser::RawVisitRecord;

pub struct FirefoxAdapter;

impl BrowserAdapter for FirefoxAdapter {
    fn browser_id(&self) -> &'static str {
        "firefox"
    }

    fn display_name(&self) -> &'static str {
        "Mozilla Firefox"
    }

    fn process_name(&self) -> &'static str {
        "firefox.exe"
    }

    fn user_data_dir(&self) -> Option<PathBuf> {
        env::var_os("APPDATA").map(|app_data| {
            PathBuf::from(app_data)
                .join("Mozilla")
                .join("Firefox")
                .join("Profiles")
        })
    }

    fn detect(&self) -> Vec<DiscoveredProfile> {
        let is_running = self.is_running();
        let profiles_dir = match self.user_data_dir() {
            Some(d) if d.exists() => d,
            _ => return Vec::new(),
        };

        let mut results = Vec::new();
        let entries = match fs::read_dir(&profiles_dir) {
            Ok(e) => e,
            Err(_) => return results,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let profile_dir_name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };

            let places_path = path.join("places.sqlite");
            if !places_path.exists() {
                continue;
            }

            let metadata = match fs::metadata(&places_path) {
                Ok(m) => m,
                Err(_) => continue,
            };

            let history_size_bytes = metadata.len();
            let last_modified = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64);

            let profile_name = if profile_dir_name.contains('.') {
                profile_dir_name
                    .split('.')
                    .nth(1)
                    .unwrap_or(&profile_dir_name)
                    .to_string()
            } else {
                profile_dir_name.clone()
            };

            results.push(DiscoveredProfile {
                source_id: None,
                browser: self.browser_id().to_string(),
                profile_id: profile_dir_name,
                profile_name,
                history_path: places_path.to_string_lossy().to_string(),
                history_size_bytes,
                last_modified,
                is_running,
            });
        }

        results
    }
}

pub fn read_firefox_history_snapshot(
    snapshot_path: &Path,
    since_unix_ms: i64,
) -> AppResult<Vec<RawVisitRecord>> {
    let conn = Connection::open_with_flags(
        snapshot_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )?;

    // Firefox places.sqlite uses microsecond timestamps from Unix Epoch (1970-01-01)
    let since_micros = since_unix_ms * 1000;

    let mut stmt = conn.prepare(
        r#"
        SELECT hv.id, hv.visit_date, COALESCE(hv.from_visit, 0), COALESCE(hv.visit_type, 0),
               p.url, COALESCE(p.title, '')
        FROM moz_historyvisits hv
        JOIN moz_places p ON hv.place_id = p.id
        WHERE hv.visit_date >= ?1
        ORDER BY hv.visit_date ASC
        "#,
    )?;

    let rows = stmt.query_map(params![since_micros], |row| {
        let source_visit_id: i64 = row.get(0)?;
        let visit_micros: i64 = row.get(1)?;
        let from_visit: i64 = row.get(2)?;
        let transition: i64 = row.get(3)?;
        let url: String = row.get(4)?;
        let title: String = row.get(5)?;

        let visit_time_unix_ms = visit_micros / 1000;

        Ok(RawVisitRecord {
            source_visit_id,
            visit_time_unix_ms,
            from_visit,
            transition,
            visit_duration: 0,
            url,
            title,
        })
    })?;

    let mut records = Vec::new();
    for r in rows {
        records.push(r?);
    }

    info!(
        "Parsed {} raw visit records from Firefox places.sqlite: {:?}",
        records.len(),
        snapshot_path
    );

    Ok(records)
}
