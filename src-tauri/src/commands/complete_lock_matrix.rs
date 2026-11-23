//! Every registered non-lifecycle command is exercised through real Tauri mock IPC.
//! All arguments are synthetic; Locked must reject before file/network/process work.
use crate::{database::DbState, security::vault::VaultManager};
use std::sync::Arc;
use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};

#[test]
fn every_sensitive_registered_command_is_rejected_while_locked() {
    let root = std::env::temp_dir().join(format!("browsory_full_ipc_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let vault = Arc::new(VaultManager::unlocked_fixture(root.clone()));
    let state = DbState {
        _directory_lock: None,
        vault: vault.clone(),
        app_dir: root.clone(),
        temp_dir: root.join("temp"),
        logs_dir: root.join("logs"),
    };
    let app = mock_builder()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            crate::commands::ai::get_embedding_status,
            crate::commands::ai::generate_embeddings_batch,
            crate::commands::ai::hybrid_search,
            crate::commands::ai::get_similar_pages,
            crate::commands::ai::get_interest_evolution,
            crate::commands::ai::get_all_topics,
            crate::commands::ai::generate_ai_period_comparison,
            crate::commands::archive::save_offline_archive,
            crate::commands::archive::get_offline_archive,
            crate::commands::archive::list_offline_archives,
            crate::commands::archive::delete_offline_archive,
            crate::commands::archive::list_background_jobs,
            crate::commands::archive::cancel_background_job,
            crate::commands::archive_files::list_archive_files,
            crate::commands::archive_files::preview_archive_file,
            crate::commands::archive_files::export_archive_file,
            crate::commands::archive_retirement::list_legacy_archive_sources,
            crate::commands::archive_retirement::retire_legacy_archive_source,
            crate::commands::auto_sync::get_auto_sync_status,
            crate::commands::auto_sync::update_auto_sync_config,
            crate::commands::auto_sync::run_auto_sync_now,
            crate::commands::auto_sync::repair_auto_sync_task,
            crate::commands::auto_sync::list_auto_sync_runs,
            crate::commands::browser::scan_browsers,
            crate::commands::browser::check_browser_running,
            crate::commands::browser::kill_browser,
            crate::commands::browser::toggle_source_enabled,
            crate::commands::browser::open_path_in_folder,
            crate::commands::browser::import_history_file,
            crate::commands::browser::import_history_batch,
            crate::commands::diagnostics::get_diagnostics_info,
            crate::commands::diagnostics::export_diagnostics_bundle,
            crate::commands::history::get_history_page,
            crate::commands::history::get_visit_detail,
            crate::commands::history::delete_visits,
            crate::commands::history::get_analytics,
            crate::commands::history::export_history,
            crate::commands::history::toggle_favorite,
            crate::commands::history::list_tags,
            crate::commands::history::add_tag_to_url,
            crate::commands::history::remove_tag_from_url,
            crate::commands::history::get_research_sessions,
            crate::commands::history::get_website_ranking,
            crate::commands::history::get_domain_detail,
            crate::commands::history::get_domain_dynamics,
            crate::commands::history::get_on_this_day,
            crate::commands::history::list_smart_collections,
            crate::commands::history::create_smart_collection,
            crate::commands::history::delete_smart_collection,
            crate::commands::history::ask_web_memory,
            crate::commands::history::get_resume_suggestion,
            crate::commands::history::import_takeout_file,
            crate::commands::history::check_link_health,
            crate::commands::history::get_link_health,
            crate::commands::security::get_plaintext_residue,
            crate::commands::security::retire_legacy_plaintext_backup,
            crate::commands::security::get_security_state,
            crate::commands::security::verify_pin,
            crate::commands::security::set_or_change_pin,
            crate::commands::security::verify_recovery_key,
            crate::commands::security::reset_pin_with_recovery_key,
            crate::commands::security::disable_pin,
            crate::commands::security::update_security_options,
            crate::commands::security::list_privacy_rules,
            crate::commands::security::add_privacy_rule,
            crate::commands::security::toggle_privacy_rule,
            crate::commands::security::delete_privacy_rule,
            crate::commands::security::create_database_backup,
            crate::commands::security::list_backups,
            crate::commands::security::delete_backup,
            crate::commands::security::restore_database_backup,
            crate::commands::security::restore_database_backup_file,
            crate::commands::security::export_database_backup,
            crate::commands::security::check_database_integrity,
            crate::commands::security::vacuum_database,
            crate::commands::settings::get_setting,
            crate::commands::settings::set_setting,
            crate::commands::settings::call_ai_completion,
            crate::commands::settings::test_ai_connection,
            crate::commands::settings::test_embedding_connection,
            crate::commands::storage::get_storage_breakdown,
            crate::commands::storage::clean_storage_cache,
            crate::commands::sync::sync_source,
            crate::commands::sync::sync_all,
            crate::commands::sync::get_recent_import_jobs,
            crate::commands::sync::test_webdav_sync,
            crate::commands::sync::execute_webdav_sync,
        ])
        .build(mock_context(noop_assets()))
        .unwrap();
    let webview =
        tauri::WebviewWindowBuilder::new(&app, "full-security-matrix", Default::default())
            .build()
            .unwrap();
    vault.lock().unwrap();
    let cases = &[
        ("get_embedding_status", serde_json::json!({"model":null})),
        (
            "generate_embeddings_batch",
            serde_json::json!({"model":null,"batchSize":null}),
        ),
        (
            "hybrid_search",
            serde_json::json!({"query":"must-not-be-used","limit":null}),
        ),
        (
            "get_similar_pages",
            serde_json::json!({"urlId":1,"model":null,"limit":null}),
        ),
        ("get_interest_evolution", serde_json::json!({"days":null})),
        ("get_all_topics", serde_json::json!({})),
        (
            "generate_ai_period_comparison",
            serde_json::json!({"days":null}),
        ),
        (
            "save_offline_archive",
            serde_json::json!({"payload":{"url_id":1,"archive_level":"full","markdown":"synthetic"}}),
        ),
        ("get_offline_archive", serde_json::json!({"urlId":1})),
        (
            "list_offline_archives",
            serde_json::json!({"limit":null,"offset":null}),
        ),
        (
            "delete_offline_archive",
            serde_json::json!({"pageUuid":"must-not-be-used"}),
        ),
        ("list_background_jobs", serde_json::json!({"limit":null})),
        (
            "cancel_background_job",
            serde_json::json!({"jobId":"must-not-be-used"}),
        ),
        (
            "list_archive_files",
            serde_json::json!({"pageUuid":"must-not-be-used"}),
        ),
        (
            "preview_archive_file",
            serde_json::json!({"pageUuid":"must-not-be-used","relativeName":"must-not-be-used"}),
        ),
        (
            "export_archive_file",
            serde_json::json!({"pageUuid":"must-not-be-used","relativeName":"must-not-be-used","targetPath":"must-not-be-used"}),
        ),
        ("list_legacy_archive_sources", serde_json::json!({})),
        (
            "retire_legacy_archive_source",
            serde_json::json!({"pageUuid":"must-not-be-used","password":"must-not-be-used","confirmation":"must-not-be-used"}),
        ),
        ("scan_browsers", serde_json::json!({})),
        (
            "check_browser_running",
            serde_json::json!({"browser":"must-not-be-used"}),
        ),
        (
            "kill_browser",
            serde_json::json!({"browser":"must-not-be-used","force":false}),
        ),
        (
            "toggle_source_enabled",
            serde_json::json!({"sourceId":1,"enabled":false}),
        ),
        (
            "open_path_in_folder",
            serde_json::json!({"path":"must-not-be-used"}),
        ),
        (
            "import_history_file",
            serde_json::json!({"filePath":"must-not-be-used","browserName":null,"profileName":null}),
        ),
        ("import_history_batch", serde_json::json!({"filePaths":[]})),
        ("get_diagnostics_info", serde_json::json!({})),
        (
            "export_diagnostics_bundle",
            serde_json::json!({"targetPath":null}),
        ),
        ("get_history_page", serde_json::json!({"filter":{}})),
        ("get_visit_detail", serde_json::json!({"visitId":1})),
        ("delete_visits", serde_json::json!({"visitIds":[]})),
        (
            "get_analytics",
            serde_json::json!({"days":null,"startTime":null,"endTime":null}),
        ),
        (
            "export_history",
            serde_json::json!({"options":{"format":"csv","file_path":"must-not-be-created.csv"}}),
        ),
        ("toggle_favorite", serde_json::json!({"urlId":1})),
        ("list_tags", serde_json::json!({})),
        (
            "add_tag_to_url",
            serde_json::json!({"urlId":1,"tagName":"must-not-be-used"}),
        ),
        (
            "remove_tag_from_url",
            serde_json::json!({"urlId":1,"tagName":"must-not-be-used"}),
        ),
        (
            "get_research_sessions",
            serde_json::json!({"days":null,"limit":null}),
        ),
        (
            "get_website_ranking",
            serde_json::json!({"startTime":null,"endTime":null,"limit":null}),
        ),
        (
            "get_domain_detail",
            serde_json::json!({"domain":"must-not-be-used","startTime":null,"endTime":null}),
        ),
        (
            "get_domain_dynamics",
            serde_json::json!({"startTime":null,"endTime":null}),
        ),
        (
            "get_on_this_day",
            serde_json::json!({"targetDate":null,"year":null,"page":null,"pageSize":null}),
        ),
        ("list_smart_collections", serde_json::json!({})),
        (
            "create_smart_collection",
            serde_json::json!({"name":"must-not-be-used","query":"must-not-be-used","filterJson":null}),
        ),
        ("delete_smart_collection", serde_json::json!({"id":1})),
        (
            "ask_web_memory",
            serde_json::json!({"question":"must-not-be-used"}),
        ),
        ("get_resume_suggestion", serde_json::json!({})),
        (
            "import_takeout_file",
            serde_json::json!({"filePath":"must-not-be-used"}),
        ),
        ("check_link_health", serde_json::json!({"urlId":1})),
        ("get_link_health", serde_json::json!({"urlId":1})),
        ("get_plaintext_residue", serde_json::json!({})),
        (
            "retire_legacy_plaintext_backup",
            serde_json::json!({"password":"must-not-be-used","confirmation":"must-not-be-used"}),
        ),
        ("get_security_state", serde_json::json!({})),
        ("verify_pin", serde_json::json!({"pin":"must-not-be-used"})),
        (
            "set_or_change_pin",
            serde_json::json!({"oldPin":null,"newPin":"must-not-be-used"}),
        ),
        (
            "verify_recovery_key",
            serde_json::json!({"recoveryKey":"must-not-be-used"}),
        ),
        (
            "reset_pin_with_recovery_key",
            serde_json::json!({"recoveryKey":"must-not-be-used","newPin":"must-not-be-used"}),
        ),
        (
            "disable_pin",
            serde_json::json!({"currentPin":"must-not-be-used"}),
        ),
        (
            "update_security_options",
            serde_json::json!({"autoLockMinutes":1,"lockOnMinimize":false,"lockOnSleep":false}),
        ),
        ("list_privacy_rules", serde_json::json!({})),
        (
            "add_privacy_rule",
            serde_json::json!({"pattern":"must-not-be-used","ruleType":"must-not-be-used"}),
        ),
        (
            "toggle_privacy_rule",
            serde_json::json!({"id":1,"enabled":false}),
        ),
        ("delete_privacy_rule", serde_json::json!({"id":1})),
        ("create_database_backup", serde_json::json!({"notes":null})),
        ("list_backups", serde_json::json!({})),
        ("delete_backup", serde_json::json!({"id":1})),
        (
            "restore_database_backup",
            serde_json::json!({"backupId":1,"password":null,"recoveryKey":null}),
        ),
        (
            "restore_database_backup_file",
            serde_json::json!({"filePath":"must-not-be-used","password":null,"recoveryKey":null}),
        ),
        (
            "export_database_backup",
            serde_json::json!({"backupId":1,"targetFilePath":"must-not-be-used"}),
        ),
        ("check_database_integrity", serde_json::json!({})),
        ("vacuum_database", serde_json::json!({})),
        ("get_setting", serde_json::json!({"key":"must-not-be-used"})),
        (
            "set_setting",
            serde_json::json!({"key":"must-not-be-used","value":"must-not-be-used"}),
        ),
        (
            "call_ai_completion",
            serde_json::json!({"payload":{"prompt":"synthetic"}}),
        ),
        (
            "test_ai_connection",
            serde_json::json!({"payload":{"base_url":"http://127.0.0.1:9","api_key":null,"model":"synthetic"}}),
        ),
        (
            "test_embedding_connection",
            serde_json::json!({"payload":{"base_url":"http://127.0.0.1:9","api_key":null,"model":"synthetic"}}),
        ),
        ("get_storage_breakdown", serde_json::json!({})),
        ("clean_storage_cache", serde_json::json!({})),
        ("sync_source", serde_json::json!({"sourceId":1})),
        ("sync_all", serde_json::json!({})),
        ("get_recent_import_jobs", serde_json::json!({"limit":null})),
        (
            "test_webdav_sync",
            serde_json::json!({"config":{"enabled":false,"server_url":"http://127.0.0.1:9","username":"synthetic","password":"synthetic"}}),
        ),
        (
            "execute_webdav_sync",
            serde_json::json!({"config":{"enabled":false,"server_url":"http://127.0.0.1:9","username":"synthetic","password":"synthetic"}}),
        ),
        ("get_auto_sync_status", serde_json::json!({})),
        (
            "update_auto_sync_config",
            serde_json::json!({"enabled":false,"intervalDays":30}),
        ),
        ("run_auto_sync_now", serde_json::json!({})),
        ("repair_auto_sync_task", serde_json::json!({})),
        ("list_auto_sync_runs", serde_json::json!({"limit":null})),
    ];
    let public: &[&str] = &[
        "change_vault_password",
        "check_for_updates",
        "confirm_vault_recovery_key",
        "create_vault",
        "get_app_info",
        "get_vault_status",
        "lock_vault",
        "migrate_legacy_database",
        "quit_application",
        "regenerate_vault_recovery_key",
        "regenerate_passive_vault_recovery_key",
        "resume_passive_vault",
        "unlock_vault",
        "unlock_vault_with_recovery_key",
    ];
    // Deliberately enumerate the production registry, rather than letting a new
    // command disappear from the test when its implementation lacks a guard.
    let source = include_str!("../lib.rs");
    let registry = source
        .split(".invoke_handler(tauri::generate_handler![")
        .nth(1)
        .unwrap()
        .split("])")
        .next()
        .unwrap();
    for name in registry
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        assert!(
            public.contains(&name) || cases.iter().any(|(covered, _)| *covered == name),
            "registered command lacks a security classification/test: {name}"
        );
    }
    for (name, body) in cases {
        let request = tauri::webview::InvokeRequest {
            cmd: (*name).into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "http://tauri.localhost".parse().unwrap(),
            body: tauri::ipc::InvokeBody::Json(body.clone()),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.into(),
        };
        let error = get_ipc_response(&webview, request)
            .expect_err(name)
            .to_string();
        assert!(
            error.contains("保险库"),
            "{name} must reject at the vault guard, got {error}"
        );
    }
    assert!(!root.join("temp").exists());
    drop(webview);
    drop(app);
    drop(vault);
    std::fs::remove_dir_all(root).unwrap();
}
