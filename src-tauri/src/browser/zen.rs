use std::env;
use std::path::PathBuf;

use super::{firefox::detect_firefox_profiles, BrowserAdapter, DiscoveredProfile};

pub struct ZenAdapter;

impl BrowserAdapter for ZenAdapter {
    fn browser_id(&self) -> &'static str {
        "zen"
    }

    fn display_name(&self) -> &'static str {
        "Zen Browser"
    }

    fn process_name(&self) -> &'static str {
        "zen.exe"
    }

    fn user_data_dir(&self) -> Option<PathBuf> {
        env::var_os("APPDATA").and_then(|app_data| {
            let base = PathBuf::from(app_data);
            let zen_lower = base.join("zen");
            if zen_lower.exists() {
                Some(zen_lower)
            } else {
                let zen_upper = base.join("Zen");
                if zen_upper.exists() {
                    Some(zen_upper)
                } else {
                    None
                }
            }
        })
    }

    fn detect(&self) -> Vec<DiscoveredProfile> {
        let is_running = self.is_running();
        if let Some(base_dir) = self.user_data_dir() {
            detect_firefox_profiles(self.browser_id(), &base_dir, is_running)
        } else {
            Vec::new()
        }
    }
}
