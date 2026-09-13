use std::env;
use std::path::PathBuf;

use super::{chromium::detect_chromium_profiles, BrowserAdapter, DiscoveredProfile};

pub struct VivaldiAdapter;

impl BrowserAdapter for VivaldiAdapter {
    fn browser_id(&self) -> &'static str {
        "vivaldi"
    }

    fn display_name(&self) -> &'static str {
        "Vivaldi"
    }

    fn process_name(&self) -> &'static str {
        "vivaldi.exe"
    }

    fn user_data_dir(&self) -> Option<PathBuf> {
        env::var_os("LOCALAPPDATA").map(|local_app_data| {
            PathBuf::from(local_app_data)
                .join("Vivaldi")
                .join("User Data")
        })
    }

    fn detect(&self) -> Vec<DiscoveredProfile> {
        let is_running = self.is_running();
        if let Some(user_data) = self.user_data_dir() {
            detect_chromium_profiles(self.browser_id(), &user_data, is_running)
        } else {
            Vec::new()
        }
    }
}
