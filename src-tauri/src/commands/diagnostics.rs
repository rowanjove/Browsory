use chrono::Utc;
use tauri::State;

use crate::database::connection::DbState;
use crate::diagnostics::{
    collect_diagnostics_info, export_diagnostics_bundle as export_bundle_fn, DiagnosticsInfo,
};
use crate::error::AppResult;

#[tauri::command]
pub async fn get_diagnostics_info(db_state: State<'_, DbState>) -> AppResult<DiagnosticsInfo> {
    let conn = db_state
        .conn
        .lock()
        .map_err(|e| crate::error::AppError::System(format!("Lock error: {}", e)))?;
    let db_path = db_state.app_dir.join("archive.db");
    collect_diagnostics_info(&conn, &db_path)
}

#[tauri::command]
pub async fn export_diagnostics_bundle(
    target_path: Option<String>,
    db_state: State<'_, DbState>,
) -> AppResult<String> {
    let conn = db_state
        .conn
        .lock()
        .map_err(|e| crate::error::AppError::System(format!("Lock error: {}", e)))?;
    let db_path = db_state.app_dir.join("archive.db");

    let out_path = if let Some(p) = target_path {
        std::path::PathBuf::from(p)
    } else {
        let now_str = Utc::now().format("%Y%m%d_%H%M%S").to_string();
        let downloads = directories::UserDirs::new()
            .and_then(|u| u.download_dir().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| db_state.app_dir.clone());
        downloads.join(format!("browsory-diagnostics-{}.zip", now_str))
    };

    export_bundle_fn(&conn, &db_path, &db_state.logs_dir, &out_path)
}
