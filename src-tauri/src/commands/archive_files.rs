use std::path::PathBuf;

use tauri::State;

use crate::archive::attachments::{self, ArchiveFileInfo, ArchiveFilePreview};
use crate::database::DbState;
use crate::error::AppResult;

#[tauri::command]
pub async fn list_archive_files(
    page_uuid: String,
    db: State<'_, DbState>,
) -> AppResult<Vec<ArchiveFileInfo>> {
    let operation = db.begin_operation()?;
    super::blocking::run(move || {
        operation.check()?;

        let result = operation.with_conn(|conn| attachments::list(conn, &page_uuid));
        operation.check()?;
        result
    })
    .await?
}

#[tauri::command]
pub async fn preview_archive_file(
    page_uuid: String,
    relative_name: String,
    db: State<'_, DbState>,
) -> AppResult<ArchiveFilePreview> {
    let operation = db.begin_operation()?;
    super::blocking::run(move || {
        operation.check()?;

        let result =
            operation.with_conn(|conn| attachments::preview(conn, &page_uuid, &relative_name));
        operation.check()?;
        result
    })
    .await?
}

#[tauri::command]
pub async fn export_archive_file(
    page_uuid: String,
    relative_name: String,
    target_path: String,
    db: State<'_, DbState>,
) -> AppResult<()> {
    let operation = db.begin_operation()?;
    let app_dir = db.app_dir.clone();
    super::blocking::run(move || {
        operation.check()?;
        super::export_destination::validate(&app_dir, std::path::Path::new(&target_path))?;

        let result = operation.with_conn(|conn| {
            attachments::export_checked(
                conn,
                &page_uuid,
                &relative_name,
                &PathBuf::from(target_path),
                || operation.check(),
            )
        });
        operation.check()?;
        result
    })
    .await?
}
