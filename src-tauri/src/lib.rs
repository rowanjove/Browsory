pub mod ai;
pub mod browser;
pub mod commands;
pub mod database;
pub mod error;
pub mod export;
pub mod history;
pub mod process;
pub mod security;

use commands::ai::{
    generate_ai_period_comparison, generate_embeddings_batch, get_all_topics, get_embedding_status,
    get_interest_evolution, get_similar_pages, hybrid_search,
};
use commands::browser::{
    check_browser_running, import_history_batch, import_history_file, kill_browser,
    open_path_in_folder, scan_browsers, toggle_source_enabled,
};
use commands::history::{
    add_tag_to_url, ask_web_memory, check_link_health, create_smart_collection,
    delete_smart_collection, delete_visits, export_history, get_analytics, get_domain_detail,
    get_domain_dynamics, get_history_page, get_link_health, get_on_this_day, get_research_sessions,
    get_resume_suggestion, get_visit_detail, get_website_ranking, import_takeout_file,
    list_smart_collections, list_tags, remove_tag_from_url, toggle_favorite,
};
use commands::security::{
    add_privacy_rule, check_database_integrity, create_database_backup, delete_backup,
    delete_privacy_rule, disable_pin, export_database_backup, get_security_state, list_backups,
    list_privacy_rules, reset_pin_with_recovery_key, restore_database_backup, set_or_change_pin,
    toggle_privacy_rule, update_security_options, vacuum_database, verify_pin, verify_recovery_key,
};
use commands::settings::{
    call_ai_completion, get_app_info, get_setting, set_setting, test_ai_connection,
};
use commands::sync::{get_recent_import_jobs, sync_all, sync_source};
use database::init_database;
use tracing_subscriber::EnvFilter;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .try_init();

    // 1. Permanently bypass Windows system proxy (Clash, VPN, etc.) for loopback and tauri internal protocols
    #[cfg(target_os = "windows")]
    {
        let existing = std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").unwrap_or_default();
        let bypass_rule =
            "--proxy-bypass-list=<-loopback>;localhost;127.0.0.1;*.localhost;tauri.localhost";
        let new_args = if existing.is_empty() {
            bypass_rule.to_string()
        } else if !existing.contains("--proxy-bypass-list") {
            format!("{} {}", existing, bypass_rule)
        } else {
            existing
        };
        std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", new_args);
    }

    // 2. In debug mode, eliminate race condition by waiting for Vite dev server before opening window
    #[cfg(debug_assertions)]
    {
        use std::net::TcpStream;
        use std::time::{Duration, Instant};
        let target = "127.0.0.1:1420";
        let start = Instant::now();
        let timeout = Duration::from_secs(30);
        tracing::info!("Checking if frontend dev server is ready at {}...", target);
        while start.elapsed() < timeout {
            if let Ok(addr) = target.parse() {
                if TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
                    tracing::info!("Frontend dev server is ready! Starting Tauri app.");
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(150));
        }
    }

    let db_state = init_database().expect("Failed to initialize SQLite archive database");

    // Initialize intelligent background History file watcher & periodic fallback sweep
    match history::watcher::start_history_watcher(db_state.clone()) {
        Ok(_handle) => {
            // Keep handle alive by leaking or managing in Tauri state
            Box::leak(Box::new(_handle));
            tracing::info!("Intelligent History Watcher running in background.");
        }
        Err(e) => {
            tracing::warn!("Failed to start intelligent history watcher: {}", e);
        }
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(db_state)
        .invoke_handler(tauri::generate_handler![
            scan_browsers,
            check_browser_running,
            kill_browser,
            toggle_source_enabled,
            open_path_in_folder,
            import_history_file,
            import_history_batch,
            sync_source,
            sync_all,
            get_history_page,
            get_visit_detail,
            delete_visits,
            get_analytics,
            export_history,
            get_app_info,
            get_setting,
            set_setting,
            call_ai_completion,
            test_ai_connection,
            get_security_state,
            verify_pin,
            set_or_change_pin,
            disable_pin,
            update_security_options,
            list_privacy_rules,
            add_privacy_rule,
            toggle_privacy_rule,
            delete_privacy_rule,
            create_database_backup,
            list_backups,
            delete_backup,
            restore_database_backup,
            export_database_backup,
            verify_recovery_key,
            reset_pin_with_recovery_key,
            check_database_integrity,
            vacuum_database,
            get_recent_import_jobs,
            toggle_favorite,
            list_tags,
            add_tag_to_url,
            remove_tag_from_url,
            get_research_sessions,
            ask_web_memory,
            get_resume_suggestion,
            get_website_ranking,
            get_domain_detail,
            get_domain_dynamics,
            get_on_this_day,
            list_smart_collections,
            create_smart_collection,
            delete_smart_collection,
            get_embedding_status,
            generate_embeddings_batch,
            hybrid_search,
            get_similar_pages,
            get_interest_evolution,
            get_all_topics,
            generate_ai_period_comparison,
            import_takeout_file,
            check_link_health,
            get_link_health,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
