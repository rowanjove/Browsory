pub mod brave;
pub mod chrome;
pub mod chromium;
pub mod edge;
pub mod firefox;
pub mod vivaldi;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredProfile {
    pub source_id: Option<i64>,
    pub browser: String,
    pub profile_id: String,
    pub profile_name: String,
    pub history_path: String,
    pub history_size_bytes: u64,
    pub last_modified: Option<i64>,
    pub is_running: bool,
}

pub trait BrowserAdapter: Send + Sync {
    fn browser_id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn process_name(&self) -> &'static str;
    fn user_data_dir(&self) -> Option<PathBuf>;
    fn detect(&self) -> Vec<DiscoveredProfile>;
    fn is_running(&self) -> bool {
        crate::process::is_process_running(self.process_name())
    }
}

pub fn get_all_adapters() -> Vec<Box<dyn BrowserAdapter>> {
    vec![
        Box::new(chrome::ChromeAdapter),
        Box::new(edge::EdgeAdapter),
        Box::new(brave::BraveAdapter),
        Box::new(vivaldi::VivaldiAdapter),
        Box::new(firefox::FirefoxAdapter),
    ]
}

pub fn detect_all_browsers() -> Vec<DiscoveredProfile> {
    let mut results = Vec::new();
    for adapter in get_all_adapters() {
        results.extend(adapter.detect());
    }
    results
}
