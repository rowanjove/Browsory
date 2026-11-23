use crate::{
    archive::retirement::{self, RetiredPage, SourcePage},
    database::DbState,
    error::AppResult,
};
use tauri::State;

#[tauri::command]
pub async fn list_legacy_archive_sources(
    offset: Option<u32>,
    db: State<'_, DbState>,
) -> AppResult<Vec<SourcePage>> {
    let operation = db.begin_operation()?;
    let db = db.inner().clone();
    super::blocking::run(move || {
        operation.check()?;

        operation.with_conn(|conn| {
            retirement::inspect_page(conn, &db.app_dir, offset.unwrap_or(0) as usize, 20, || {
                operation.check()
            })
        })
    })
    .await?
}

#[tauri::command]
pub async fn retire_legacy_archive_source(
    page_uuid: String,
    password: String,
    confirmation: String,
    db: State<'_, DbState>,
) -> Result<RetiredPage, String> {
    let operation = db.begin_operation().map_err(|e| e.to_string())?;
    let key = db.vault.master_key().ok_or("保险库主密钥不可用")?;
    let envelope = db.vault.envelope().ok_or("保险库信封不可用")?;
    let root = db.app_dir.clone();
    let password = zeroize::Zeroizing::new(password);
    tauri::async_runtime::spawn_blocking(move || {
        let handle = operation.connection()?;
        let conn = handle.lock()?;
        // retire owns its final session check before the irreversible commit.
        retirement::retire(
            &conn,
            &root,
            &key,
            &envelope,
            (&page_uuid, &password, &confirmation),
            || operation.check(),
        )
    })
    .await
    .map_err(|_| "旧网页退役任务中断，请检查源目录与恢复副本状态".to_string())?
    .map_err(|e: crate::error::AppError| e.to_string())
}
