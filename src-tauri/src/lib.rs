pub mod ai;
pub mod archive;
pub mod auto_sync;
pub mod browser;
pub mod commands;
pub mod database;
pub mod diagnostics;
pub mod error;
pub mod export;
pub mod history;
pub mod logging;
pub mod process;
pub mod security;
pub mod sync;
pub mod tray;

use commands::archive::{
    cancel_background_job, delete_offline_archive, get_offline_archive, list_background_jobs,
    list_offline_archives, save_offline_archive,
};

use commands::ai::{
    generate_ai_period_comparison, generate_embeddings_batch, get_all_topics, get_embedding_status,
    get_interest_evolution, get_similar_pages, hybrid_search,
};
use commands::archive_files::{export_archive_file, list_archive_files, preview_archive_file};
use commands::archive_retirement::{list_legacy_archive_sources, retire_legacy_archive_source};
use commands::auto_sync::{
    get_auto_sync_status, list_auto_sync_runs, repair_auto_sync_task, run_auto_sync_now,
    update_auto_sync_config,
};
use commands::browser::{
    check_browser_running, import_history_batch, import_history_file, kill_browser,
    open_path_in_folder, scan_browsers, toggle_source_enabled,
};
use commands::diagnostics::{export_diagnostics_bundle, get_diagnostics_info};
use commands::history::{
    add_tag_to_url, ask_web_memory, check_link_health, create_smart_collection,
    delete_smart_collection, delete_visits, export_history, get_analytics, get_domain_detail,
    get_domain_dynamics, get_history_page, get_link_health, get_on_this_day, get_research_sessions,
    get_resume_suggestion, get_visit_detail, get_website_ranking, import_takeout_file,
    list_smart_collections, list_tags, remove_tag_from_url, toggle_favorite,
};
use commands::security::{
    add_privacy_rule, change_vault_password, check_database_integrity, confirm_vault_recovery_key,
    create_database_backup, create_vault, delete_backup, delete_privacy_rule, disable_pin,
    export_database_backup, get_plaintext_residue, get_security_state, get_vault_status,
    list_backups, list_privacy_rules, lock_vault, migrate_legacy_database,
    regenerate_passive_vault_recovery_key, regenerate_vault_recovery_key,
    reset_pin_with_recovery_key, restore_database_backup, restore_database_backup_file,
    resume_passive_vault, retire_legacy_plaintext_backup, set_or_change_pin, toggle_privacy_rule,
    unlock_vault, unlock_vault_with_recovery_key, update_security_options, vacuum_database,
    verify_pin, verify_recovery_key,
};
use commands::settings::{
    call_ai_completion, check_for_updates, get_app_info, get_setting, quit_application,
    set_setting, test_ai_connection, test_embedding_connection,
};
use commands::storage::{clean_storage_cache, get_storage_breakdown};
use commands::sync::{
    execute_webdav_sync, get_recent_import_jobs, sync_all, sync_source, test_webdav_sync,
};
use database::init_database;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 0. Initialize app directories and rolling logging
    let db_state = match init_database() {
        Ok(state) => state,
        Err(e) => {
            eprintln!("Failed to initialize app directories: {}", e);
            return;
        }
    };
    let _log_guard = logging::setup_logging(&db_state.logs_dir);

    // 1. Permanently bypass Windows system proxy (Clash, VPN, etc.) for loopback and tauri internal protocols
    #[cfg(target_os = "windows")]
    {
        let existing = std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").unwrap_or_default();
        let bypass_rule =
            "--proxy-bypass-list=<loopback>;<local>;localhost;127.0.0.1;*.localhost;tauri.localhost";
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

    // Milestone D: Run post-unlock lifecycle checks (automated backup & legacy cleanup) on passive startup
    let db_backup_state = db_state.clone();
    std::thread::spawn(move || {
        // Wait 4 seconds for UI startup and initial read-locks to settle
        std::thread::sleep(std::time::Duration::from_secs(4));
        if db_backup_state.vault.is_unlocked() {
            commands::security::post_unlock_lifecycle(&db_backup_state);
        }
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            use tauri::Manager;
            history::watcher::set_app_events(app.handle().clone());
            match security::power::install(app.handle().clone()) {
                Ok(subscription) => {
                    app.manage::<security::power::PowerSubscription>(subscription);
                }
                Err(error) => tracing::warn!("Native sleep locking is unavailable: {error}"),
            }
            // Setup system tray
            if let Err(e) = tray::setup_tray(app.handle()) {
                tracing::warn!("Failed to setup system tray: {}", e);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            use tauri::{Emitter, Manager};
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let behavior = window
                    .try_state::<database::DbState>()
                    .and_then(|db| {
                        db.get_connection().ok().and_then(|handle| {
                            // If the database is busy, show the close dialog
                            // instead of blocking the native window event loop.
                            handle.try_lock().ok().flatten().and_then(|conn| {
                                database::repository::get_setting(&conn, "close_behavior")
                                    .ok()
                                    .flatten()
                            })
                        })
                    })
                    .unwrap_or_default();
                match behavior.as_str() {
                    "tray" => {
                        let _ = window.hide();
                    }
                    "quit" => {
                        window.app_handle().exit(0);
                    }
                    _ => {
                        let _ = window.emit("browsory://close-requested", ());
                    }
                }
            }
        })
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
            quit_application,
            check_for_updates,
            call_ai_completion,
            test_ai_connection,
            get_vault_status,
            get_plaintext_residue,
            retire_legacy_plaintext_backup,
            create_vault,
            unlock_vault,
            resume_passive_vault,
            unlock_vault_with_recovery_key,
            lock_vault,
            change_vault_password,
            confirm_vault_recovery_key,
            regenerate_vault_recovery_key,
            regenerate_passive_vault_recovery_key,
            migrate_legacy_database,
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
            restore_database_backup_file,
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
            test_embedding_connection,
            get_diagnostics_info,
            export_diagnostics_bundle,
            get_storage_breakdown,
            clean_storage_cache,
            save_offline_archive,
            list_archive_files,
            preview_archive_file,
            export_archive_file,
            list_legacy_archive_sources,
            retire_legacy_archive_source,
            get_offline_archive,
            list_offline_archives,
            delete_offline_archive,
            list_background_jobs,
            cancel_background_job,
            test_webdav_sync,
            execute_webdav_sync,
            get_auto_sync_status,
            update_auto_sync_config,
            run_auto_sync_now,
            repair_auto_sync_task,
            list_auto_sync_runs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
