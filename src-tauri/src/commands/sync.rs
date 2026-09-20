use std::path::PathBuf;
use tauri::State;
use tracing::{debug, info};

use crate::database::models::SyncResult;
use crate::database::repository::list_sources;
use crate::database::DbState;
use crate::error::AppResult;
use crate::history::sync::sync_source_history;

#[tauri::command]
pub fn sync_source(source_id: i64, db: State<'_, DbState>) -> AppResult<SyncResult> {
    let mut conn = db.conn.lock().unwrap();

    let source = {
        let mut stmt = conn.prepare(
            r#"
            SELECT id, browser, profile, source_type, history_path,
                   db_fingerprint, last_visit_id, last_visit_time, last_sync_at, enabled
            FROM sources WHERE id = ?1
            "#,
        )?;

        stmt.query_row([source_id], |row| {
            Ok(crate::database::models::Source {
                id: row.get(0)?,
                browser: row.get(1)?,
                profile: row.get(2)?,
                source_type: row.get(3)?,
                history_path: row.get(4)?,
                db_fingerprint: row.get(5)?,
                last_visit_id: row.get(6)?,
                last_visit_time: row.get(7)?,
                last_sync_at: row.get(8)?,
                enabled: row.get::<_, i64>(9)? == 1,
            })
        })?
    };

    let history_path = PathBuf::from(&source.history_path);
    sync_source_history(
        &mut conn,
        source.id,
        &source.browser,
        &source.profile,
        &history_path,
        source.last_visit_time,
        &db.temp_dir,
    )
}

#[tauri::command]
pub fn sync_all(db: State<'_, DbState>) -> AppResult<Vec<SyncResult>> {
    let sources = {
        let conn = db.conn.lock().unwrap();
        list_sources(&conn)?
    };

    let mut results = Vec::new();

    for source in sources {
        if !source.enabled {
            continue;
        }

        let history_path = PathBuf::from(&source.history_path);
        if !history_path.exists() {
            continue;
        }

        // 1. Fast incremental check: if file and WAL have not changed since last sync, skip with 0 disk I/O!
        if !crate::history::sync::is_source_modified_since_last_sync(
            &history_path,
            source.last_sync_at,
        ) {
            debug!(
                "Source #{}: File {:?} unchanged since last sync, skipping.",
                source.id, history_path
            );
            results.push(SyncResult {
                source_id: source.id,
                browser: source.browser.clone(),
                profile: source.profile.clone(),
                read_count: 0,
                inserted_count: 0,
                duplicate_count: 0,
                failed_count: 0,
                duration_ms: 0,
            });
            continue;
        }

        let start_time = std::time::Instant::now();

        // 2. Prepare snapshot & parse records WITHOUT holding the database lock
        let prepared = match crate::history::sync::prepare_source_records(
            source.id,
            &source.browser,
            &history_path,
            source.last_visit_time,
            source.db_fingerprint.as_deref(),
            &db.temp_dir,
        ) {
            Ok(data) => data,
            Err(e) => {
                info!(
                    "Sync preparation failed for source #{}: {}: {}",
                    source.id, source.browser, e
                );
                continue;
            }
        };

        // 3. Commit records to archive.db holding lock ONLY during this brief transaction
        let commit_res = {
            let mut conn = db.conn.lock().unwrap();
            crate::history::sync::commit_source_records(
                &mut conn,
                source.id,
                &source.browser,
                &source.profile,
                source.last_visit_time,
                prepared,
                start_time,
            )
        };

        match commit_res {
            Ok(res) => results.push(res),
            Err(e) => {
                info!(
                    "Sync commit failed for source #{}: {}: {}",
                    source.id, source.browser, e
                );
            }
        }
    }

    Ok(results)
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ImportJobItem {
    pub id: i64,
    pub source_id: Option<i64>,
    pub browser: Option<String>,
    pub profile: Option<String>,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub read_count: u32,
    pub inserted_count: u32,
    pub duplicate_count: u32,
    pub failed_count: u32,
    pub status: String,
    pub error: Option<String>,
}

#[tauri::command]
pub fn get_recent_import_jobs(
    limit: Option<u32>,
    db: State<'_, DbState>,
) -> AppResult<Vec<ImportJobItem>> {
    let conn = db.conn.lock().unwrap();
    let l = limit.unwrap_or(10).min(50);
    let mut stmt = conn.prepare(
        r#"
        SELECT j.id, j.source_id, s.browser, s.profile, j.started_at, j.finished_at,
               j.read_count, j.inserted_count, j.duplicate_count, j.failed_count,
               j.status, j.error
        FROM import_jobs j
        LEFT JOIN sources s ON s.id = j.source_id
        ORDER BY j.started_at DESC
        LIMIT ?1
        "#,
    )?;

    let rows = stmt.query_map([l], |row| {
        Ok(ImportJobItem {
            id: row.get(0)?,
            source_id: row.get(1)?,
            browser: row.get(2)?,
            profile: row.get(3)?,
            started_at: row.get(4)?,
            finished_at: row.get(5)?,
            read_count: row.get::<_, i64>(6)? as u32,
            inserted_count: row.get::<_, i64>(7)? as u32,
            duplicate_count: row.get::<_, i64>(8)? as u32,
            failed_count: row.get::<_, i64>(9)? as u32,
            status: row.get(10)?,
            error: row.get(11)?,
        })
    })?;

    let mut jobs = Vec::new();
    for r in rows {
        jobs.push(r?);
    }
    Ok(jobs)
}

fn hydrate_webdav_password(
    mut config: crate::sync::webdav::WebDavConfig,
    app_dir: &std::path::Path,
) -> AppResult<crate::sync::webdav::WebDavConfig> {
    if config.password.trim().is_empty() {
        config.password = crate::security::secret_store::get_secret("webdav_password", app_dir)
            .map_err(|e| crate::error::AppError::Security(format!("读取 WebDAV 密码失败: {}", e)))?
            .unwrap_or_default();
    }
    Ok(config)
}

#[tauri::command]
pub async fn test_webdav_sync(
    config: crate::sync::webdav::WebDavConfig,
    db: State<'_, DbState>,
) -> AppResult<()> {
    let config = hydrate_webdav_password(config, &db.app_dir)?;
    let client = crate::sync::webdav::WebDavClient::new(config)?;
    client.test_connection().await
}

#[tauri::command]
pub async fn execute_webdav_sync(
    config: crate::sync::webdav::WebDavConfig,
    db: State<'_, DbState>,
) -> AppResult<crate::sync::SyncStatusReport> {
    let config = hydrate_webdav_password(config, &db.app_dir)?;
    if config.enabled {
        return Err(crate::error::AppError::Sync(
            "双向 WebDAV 下载、冲突合并和落库尚未完成；为避免产生无法恢复的单向密文包，已拒绝执行同步。".into(),
        ));
    }
    let sync_secret = crate::sync::get_or_create_sync_secret(&db.app_dir)?;
    let local_items = {
        let conn = db.conn.lock().unwrap();
        crate::sync::collect_local_sync_items(&conn)?
    };
    crate::sync::upload_and_sync(&db.app_dir, &config, &sync_secret, local_items).await
}
