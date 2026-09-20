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
        env::var_os("APPDATA")
            .map(|app_data| PathBuf::from(app_data).join("Mozilla").join("Firefox"))
    }

    fn detect(&self) -> Vec<DiscoveredProfile> {
        let is_running = self.is_running();
        let base_dir = match self.user_data_dir() {
            Some(d) if d.exists() => d,
            _ => return Vec::new(),
        };

        detect_firefox_profiles(self.browser_id(), &base_dir, is_running)
    }
}

/// Detects Firefox profiles by parsing profiles.ini and scanning directory candidates
pub fn detect_firefox_profiles(
    browser_id: &str,
    base_dir: &Path,
    is_running: bool,
) -> Vec<DiscoveredProfile> {
    let mut results = Vec::new();
    let mut seen_paths = std::collections::HashSet::new();

    // 1. Try parsing profiles.ini
    let ini_path = base_dir.join("profiles.ini");
    if ini_path.exists() {
        if let Ok(content) = fs::read_to_string(&ini_path) {
            let mut current_name = String::new();
            let mut current_path = String::new();
            let mut is_relative = true;

            let flush_profile =
                |name: &str,
                 path_str: &str,
                 rel: bool,
                 res: &mut Vec<DiscoveredProfile>,
                 seen: &mut std::collections::HashSet<String>| {
                    if path_str.is_empty() {
                        return;
                    }
                    let profile_path = if rel {
                        base_dir.join(path_str.replace('/', "\\"))
                    } else {
                        PathBuf::from(path_str)
                    };

                    let places_path = profile_path.join("places.sqlite");
                    if places_path.exists() && places_path.is_file() {
                        let places_str = places_path.to_string_lossy().to_string();
                        if !seen.insert(places_str.clone()) {
                            return;
                        }
                        let metadata = fs::metadata(&places_path).ok();
                        let history_size_bytes = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
                        let last_modified = metadata
                            .and_then(|m| m.modified().ok())
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_millis() as i64);

                        let folder_name = profile_path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("Default")
                            .to_string();
                        let display_name = if !name.is_empty() {
                            name.to_string()
                        } else if folder_name.contains('.') {
                            folder_name
                                .split('.')
                                .nth(1)
                                .unwrap_or(&folder_name)
                                .to_string()
                        } else {
                            folder_name.clone()
                        };

                        res.push(DiscoveredProfile {
                            source_id: None,
                            browser: browser_id.to_string(),
                            profile_id: folder_name,
                            profile_name: display_name,
                            history_path: places_str,
                            history_size_bytes,
                            last_modified,
                            is_running,
                        });
                    }
                };

            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with('[') && trimmed.ends_with(']') {
                    flush_profile(
                        &current_name,
                        &current_path,
                        is_relative,
                        &mut results,
                        &mut seen_paths,
                    );
                    current_name.clear();
                    current_path.clear();
                    is_relative = true;
                } else if let Some((k, v)) = trimmed.split_once('=') {
                    let key = k.trim();
                    let val = v.trim();
                    if key.eq_ignore_ascii_case("Name") {
                        current_name = val.to_string();
                    } else if key.eq_ignore_ascii_case("Path") {
                        current_path = val.to_string();
                    } else if key.eq_ignore_ascii_case("IsRelative") {
                        is_relative = val != "0";
                    }
                }
            }
            flush_profile(
                &current_name,
                &current_path,
                is_relative,
                &mut results,
                &mut seen_paths,
            );
        }
    }

    // 2. Fallback: scan Profiles/ directory if profiles.ini missed anything
    let profiles_dir = base_dir.join("Profiles");
    if profiles_dir.exists() && profiles_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&profiles_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }

                let places_path = path.join("places.sqlite");
                if !places_path.exists() || !places_path.is_file() {
                    continue;
                }

                let places_str = places_path.to_string_lossy().to_string();
                if !seen_paths.insert(places_str.clone()) {
                    continue;
                }

                let profile_dir_name = match path.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n.to_string(),
                    None => continue,
                };

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
                    browser: browser_id.to_string(),
                    profile_id: profile_dir_name,
                    profile_name,
                    history_path: places_str,
                    history_size_bytes,
                    last_modified,
                    is_running,
                });
            }
        }
    }

    results
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
