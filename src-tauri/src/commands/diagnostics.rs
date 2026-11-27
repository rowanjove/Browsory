use chrono::Utc;
use tauri::State;

use crate::database::connection::DbState;
use crate::diagnostics::{
    collect_diagnostics_info, collect_diagnostics_info_fast, export_diagnostics_bundle_with_info,
    DiagnosticsInfo,
};
use crate::error::AppResult;

#[tauri::command]
pub async fn get_diagnostics_info(db_state: State<'_, DbState>) -> AppResult<DiagnosticsInfo> {
    let operation = db_state.begin_operation()?;
    let db_state = db_state.inner().clone();
    super::blocking::run(move || {
        operation.check()?;
        let db_path = db_state.app_dir.join("archive.db");
        let result = operation.with_read_conn(|conn| collect_diagnostics_info_fast(conn, &db_path));
        operation.check()?;
        result
    })
    .await?
}

#[tauri::command]
pub async fn export_diagnostics_bundle(
    target_path: Option<String>,
    db_state: State<'_, DbState>,
) -> AppResult<String> {
    let operation = db_state.begin_operation()?;
    let db_state = db_state.inner().clone();
    super::blocking::run(move || {
        operation.check()?;
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

        super::export_destination::validate(&db_state.app_dir, &out_path)?;
        let info = {
            operation.check()?;
            let conn_arc = operation.connection()?;
            let conn = conn_arc
                .lock()
                .map_err(|e| crate::error::AppError::System(format!("Lock error: {}", e)))?;
            let result = collect_diagnostics_info(&conn, &db_path)?;
            operation.check()?;
            result
        };
        let result = export_diagnostics_bundle_with_info(&info, &db_state.logs_dir, &out_path);
        operation.check()?;
        result
    })
    .await?
}
