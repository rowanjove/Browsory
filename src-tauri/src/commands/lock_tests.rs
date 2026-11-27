//! Actual command dispatch over Tauri's mock IPC runtime, using a synthetic
//! SQLCipher database. No desktop app data directory or external services.
use std::sync::Arc;
use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};

use crate::database::{DbState, HistoryFilter};
use crate::security::vault::VaultManager;

fn request(command: &str, body: serde_json::Value) -> tauri::webview::InvokeRequest {
    tauri::webview::InvokeRequest {
        cmd: command.into(),
        callback: tauri::ipc::CallbackFn(0),
        error: tauri::ipc::CallbackFn(1),
        url: "http://tauri.localhost".parse().unwrap(),
        body: tauri::ipc::InvokeBody::Json(body),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.into(),
    }
}

#[test]
fn imported_backup_uses_source_credentials_locks_and_preserves_latest_target_wal() {
    use super::security;
    use crate::security::backup::execute_encrypted_backup;
    use crate::security::vault::VaultStatus;
    let root = std::env::temp_dir().join(format!("browsory_imported_ipc_{}", uuid::Uuid::new_v4()));
    let source = root.join("source");
    let target = root.join("target");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&target).unwrap();
    let original = VaultManager::unlocked_fixture(source.clone());
    original.with_conn(|conn| {
        conn.execute_batch("CREATE TABLE imported_sentinel(value TEXT); INSERT INTO imported_sentinel VALUES('source history');")?;
        Ok(())
    }).unwrap();
    let package = original
        .with_conn(|conn| {
            Ok(execute_encrypted_backup(
                conn,
                &source,
                &source.join("backups"),
                "test",
                None,
                &original.master_key().unwrap(),
                &original.envelope().unwrap(),
            )
            .unwrap())
        })
        .unwrap();
    original.lock().unwrap();
    let vault = Arc::new(VaultManager::unlocked_fixture(target.clone()));
    // Different key AND different password prove that restore adopts the source pair.
    vault
        .change_password("Synthetic-fixture-password", "Target-only-passphrase")
        .unwrap();
    let old_key = vault.master_key().unwrap();
    let old_envelope = vault.envelope().unwrap();
    vault.with_conn(|conn| {
        conn.query_row("PRAGMA wal_autocheckpoint=0", [], |r| r.get::<_, i64>(0))?;
        conn.execute_batch("CREATE TABLE target_sentinel(value TEXT); INSERT INTO target_sentinel VALUES('last target WAL write');")?;
        Ok(())
    }).unwrap();
    assert!(
        std::fs::metadata(target.join("archive.db-wal"))
            .unwrap()
            .len()
            > 0
    );
    let retained = vault.connection().unwrap();
    let state = DbState {
        _directory_lock: None,
        vault: vault.clone(),
        app_dir: target.clone(),
        temp_dir: target.join("temp"),
        logs_dir: target.join("logs"),
    };
    let app = mock_builder()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            security::restore_database_backup_file
        ])
        .build(mock_context(noop_assets()))
        .unwrap();
    let webview = tauri::WebviewWindowBuilder::new(&app, "restore-test", Default::default())
        .build()
        .unwrap();
    let response = get_ipc_response(
        &webview,
        request(
            "restore_database_backup_file",
            serde_json::json!({
                "filePath":package.file_path,"password":"Synthetic-fixture-password","recoveryKey":null
            }),
        ),
    );
    assert!(response.is_ok(), "{response:?}");
    assert_eq!(vault.status(), VaultStatus::Locked);
    assert!(vault.master_key().is_none());
    assert!(retained.lock().is_err());
    assert!(vault
        .unlock_with_password("Target-only-passphrase")
        .is_err());
    vault
        .unlock_with_password("Synthetic-fixture-password")
        .unwrap();
    vault
        .with_conn(|conn| {
            assert_eq!(
                conn.query_row("SELECT value FROM imported_sentinel", [], |r| r
                    .get::<_, String>(0))?,
                "source history"
            );
            Ok(())
        })
        .unwrap();
    let previous = std::fs::read_dir(target.join("backups"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(
        crate::security::envelope::VaultEnvelope::load_from_dir(&previous)
            .unwrap()
            .unwrap(),
        old_envelope
    );
    let old = crate::database::sqlcipher::open_encrypted_database(
        &previous.join("archive.db"),
        old_key.as_bytes(),
        false,
    )
    .unwrap();
    assert_eq!(
        old.query_row("SELECT value FROM target_sentinel", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "last target WAL write"
    );
    drop(old);
    vault.lock().unwrap();
    drop(webview);
    drop(app);
    drop(vault);
    drop(original);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn locked_sensitive_ipc_matrix_rejects_queries_exports_imports_and_credentials() {
    use super::{ai, archive, browser, diagnostics, history, security, settings, sync};
    let root = std::env::temp_dir().join(format!("browsory_locked_ipc_{}", uuid::Uuid::new_v4()));
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
            history::get_history_page,
            history::get_visit_detail,
            history::get_analytics,
            history::export_history,
            history::ask_web_memory,
            history::check_link_health,
            history::import_takeout_file,
            sync::sync_all,
            ai::generate_embeddings_batch,
            security::create_database_backup,
            security::restore_database_backup,
            security::restore_database_backup_file,
            security::get_plaintext_residue,
            security::retire_legacy_plaintext_backup,
            settings::get_setting,
            settings::set_setting,
            archive::get_offline_archive,
            diagnostics::get_diagnostics_info,
            browser::scan_browsers,
        ])
        .build(mock_context(noop_assets()))
        .unwrap();
    let webview = tauri::WebviewWindowBuilder::new(&app, "security-test", Default::default())
        .build()
        .unwrap();
    let filter = serde_json::to_value(HistoryFilter::default()).unwrap();
    assert!(
        get_ipc_response(
            &webview,
            request("get_history_page", serde_json::json!({"filter":filter}))
        )
        .is_ok(),
        "unlocked dispatch must reach the real query"
    );
    let retained = vault.connection().unwrap();
    vault.lock().unwrap();
    let target = root.join("must-not-be-created.csv");
    let cases = [
        ("get_history_page", serde_json::json!({"filter":filter})),
        ("get_visit_detail", serde_json::json!({"visitId":1})),
        (
            "get_analytics",
            serde_json::json!({"days":7,"startTime":null,"endTime":null}),
        ),
        (
            "export_history",
            serde_json::json!({"options":{"format":"csv","file_path":target}}),
        ),
        (
            "ask_web_memory",
            serde_json::json!({"question":"must not leave the locked vault"}),
        ),
        ("check_link_health", serde_json::json!({"urlId":1})),
        (
            "import_takeout_file",
            serde_json::json!({"filePath":"must-not-be-read.json"}),
        ),
        ("sync_all", serde_json::json!({})),
        (
            "generate_embeddings_batch",
            serde_json::json!({"model":null,"batchSize":1}),
        ),
        ("create_database_backup", serde_json::json!({"notes":null})),
        ("get_plaintext_residue", serde_json::json!({})),
        (
            "retire_legacy_plaintext_backup",
            serde_json::json!({"password":"must-not-be-used","confirmation":"移除旧明文副本"}),
        ),
        (
            "restore_database_backup",
            serde_json::json!({"backupId":1,"password":null,"recoveryKey":null}),
        ),
        (
            "restore_database_backup_file",
            serde_json::json!({"filePath":"must-not-be-read.browsory-backup","password":null,"recoveryKey":null}),
        ),
        ("get_setting", serde_json::json!({"key":"ai_key"})),
        (
            "set_setting",
            serde_json::json!({"key":"ai_key","value":"must-not-be-persisted"}),
        ),
        ("get_offline_archive", serde_json::json!({"urlId":1})),
        ("get_diagnostics_info", serde_json::json!({})),
        ("scan_browsers", serde_json::json!({})),
    ];
    for (name, body) in cases {
        let error = get_ipc_response(&webview, request(name, body))
            .expect_err(name)
            .to_string();
        assert!(
            error.contains("保险库"),
            "{name} must fail at the vault guard, got {error}"
        );
    }
    assert!(!target.exists());
    assert!(!root.join("temp").exists());
    assert!(retained.lock().is_err());
    drop(webview);
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
