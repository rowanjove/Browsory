use std::env;
use std::path::PathBuf;

use super::{chromium::detect_chromium_profiles, BrowserAdapter, DiscoveredProfile};

pub struct BraveAdapter;

impl BrowserAdapter for BraveAdapter {
    fn browser_id(&self) -> &'static str {
        "brave"
    }

    fn display_name(&self) -> &'static str {
        "Brave Browser"
    }

    fn process_name(&self) -> &'static str {
        "brave.exe"
    }

    fn user_data_dir(&self) -> Option<PathBuf> {
        env::var_os("LOCALAPPDATA").map(|local_app_data| {
            PathBuf::from(local_app_data)
                .join("BraveSoftware")
                .join("Brave-Browser")
                .join("User Data")
        })
    }

    fn detect_with_running(&self, is_running: bool) -> Vec<DiscoveredProfile> {
        if let Some(user_data) = self.user_data_dir() {
            detect_chromium_profiles(self.browser_id(), &user_data, is_running)
        } else {
            Vec::new()
        }
    }
}
