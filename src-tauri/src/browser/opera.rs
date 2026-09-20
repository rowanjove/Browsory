use std::env;
use std::path::PathBuf;

use super::{chromium::detect_chromium_profiles, BrowserAdapter, DiscoveredProfile};

pub struct OperaAdapter;

impl BrowserAdapter for OperaAdapter {
    fn browser_id(&self) -> &'static str {
        "opera"
    }

    fn display_name(&self) -> &'static str {
        "Opera"
    }

    fn process_name(&self) -> &'static str {
        "opera.exe"
    }

    fn user_data_dir(&self) -> Option<PathBuf> {
        env::var_os("APPDATA").map(|app_data| {
            PathBuf::from(app_data)
                .join("Opera Software")
                .join("Opera Stable")
        })
    }

    fn detect(&self) -> Vec<DiscoveredProfile> {
        let is_running = self.is_running();
        let mut profiles = Vec::new();

        // 1. Standard Opera Stable
        if let Some(user_data) = self.user_data_dir() {
            profiles.extend(detect_chromium_profiles(
                self.browser_id(),
                &user_data,
                is_running,
            ));
        }

        // 2. Opera GX
        if let Some(app_data) = env::var_os("APPDATA") {
            let gx_dir = PathBuf::from(app_data)
                .join("Opera Software")
                .join("Opera GX Stable");
            if gx_dir.exists() {
                let gx_running = crate::process::is_process_running("opera.exe");
                let mut gx_profiles = detect_chromium_profiles("operagx", &gx_dir, gx_running);
                for p in &mut gx_profiles {
                    p.profile_name = format!("Opera GX ({})", p.profile_name);
                }
                profiles.extend(gx_profiles);
            }
        }

        profiles
    }
}
