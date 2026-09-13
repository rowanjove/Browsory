use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tracing::debug;

use super::DiscoveredProfile;

pub fn detect_chromium_profiles(
    browser_id: &str,
    user_data_dir: &Path,
    is_running: bool,
) -> Vec<DiscoveredProfile> {
    if !user_data_dir.exists() || !user_data_dir.is_dir() {
        return Vec::new();
    }

    let mut profiles = Vec::new();

    // 1. Try to parse Local State for profile display names
    let display_names = parse_local_state(user_data_dir);

    // 2. Scan User Data directory for folders containing a History file
    if let Ok(entries) = fs::read_dir(user_data_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let folder_name = match path.file_name().and_then(|n| n.to_str()) {
                Some(name) => name.to_string(),
                None => continue,
            };

            // Check if folder is a Profile candidate (Default, Profile 1, etc.)
            let is_candidate = folder_name == "Default"
                || folder_name.starts_with("Profile ")
                || display_names.contains_key(&folder_name);

            if !is_candidate {
                continue;
            }

            let history_file = path.join("History");
            if history_file.exists() && history_file.is_file() {
                let metadata = fs::metadata(&history_file).ok();
                let history_size_bytes = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
                let last_modified = metadata
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as i64);

                let display_name = display_names.get(&folder_name).cloned().unwrap_or_else(|| {
                    if folder_name == "Default" {
                        "Default".to_string()
                    } else {
                        folder_name.clone()
                    }
                });

                profiles.push(DiscoveredProfile {
                    source_id: None,
                    browser: browser_id.to_string(),
                    profile_id: folder_name,
                    profile_name: display_name,
                    history_path: history_file.to_string_lossy().to_string(),
                    history_size_bytes,
                    last_modified,
                    is_running,
                });
            }
        }
    }

    profiles
}

fn parse_local_state(user_data_dir: &Path) -> HashMap<String, String> {
    let mut names = HashMap::new();
    let local_state_path = user_data_dir.join("Local State");
    if !local_state_path.exists() {
        return names;
    }

    let content = match fs::read_to_string(&local_state_path) {
        Ok(c) => c,
        Err(e) => {
            debug!("Failed to read Local State: {}", e);
            return names;
        }
    };

    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(j) => j,
        Err(e) => {
            debug!("Failed to parse Local State JSON: {}", e);
            return names;
        }
    };

    if let Some(info_cache) = json
        .get("profile")
        .and_then(|p| p.get("info_cache"))
        .and_then(|c| c.as_object())
    {
        for (folder, info) in info_cache {
            if let Some(name) = info.get("name").and_then(|n| n.as_str()) {
                names.insert(folder.clone(), name.to_string());
            }
        }
    }

    names
}
