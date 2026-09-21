use tauri::State;

use crate::archive::{
    delete_offline_archive as repo_delete_offline_archive,
    get_offline_archive as repo_get_offline_archive,
    list_offline_archives as repo_list_offline_archives,
    save_offline_archive as repo_save_offline_archive, PageArchiveDetail, PageArchiveSummary,
    SaveArchivePayload,
};
use crate::database::DbState;
use crate::error::AppResult;
use crate::process::jobs::{
    cancel_job as repo_cancel_job, list_jobs as repo_list_jobs, BackgroundJob,
};

#[tauri::command]
pub async fn save_offline_archive(
    payload: SaveArchivePayload,
    db: State<'_, DbState>,
) -> AppResult<PageArchiveSummary> {
    crate::license::require_pro(&db.app_dir, crate::license::FEATURE_OFFLINE_ARCHIVE)?;
    let conn = db.conn.lock().unwrap();
    repo_save_offline_archive(&db.app_dir, &conn, payload)
}

#[tauri::command]
pub async fn get_offline_archive(
    url_id: i64,
    db: State<'_, DbState>,
) -> AppResult<Option<PageArchiveDetail>> {
    let conn = db.conn.lock().unwrap();
    repo_get_offline_archive(&db.app_dir, &conn, url_id)
}

#[tauri::command]
pub async fn list_offline_archives(
    limit: Option<u32>,
    offset: Option<u32>,
    db: State<'_, DbState>,
) -> AppResult<Vec<PageArchiveSummary>> {
    let conn = db.conn.lock().unwrap();
    repo_list_offline_archives(&conn, limit.unwrap_or(50), offset.unwrap_or(0))
}

#[tauri::command]
pub async fn delete_offline_archive(page_uuid: String, db: State<'_, DbState>) -> AppResult<()> {
    let conn = db.conn.lock().unwrap();
    repo_delete_offline_archive(&db.app_dir, &conn, &page_uuid)
}

#[tauri::command]
pub fn list_background_jobs(
    limit: Option<u32>,
    db: State<'_, DbState>,
) -> AppResult<Vec<BackgroundJob>> {
    let conn = db.conn.lock().unwrap();
    repo_list_jobs(&conn, limit.unwrap_or(20))
}

#[tauri::command]
pub fn cancel_background_job(job_id: String, db: State<'_, DbState>) -> AppResult<()> {
    let conn = db.conn.lock().unwrap();
    repo_cancel_job(&conn, &job_id)
}
