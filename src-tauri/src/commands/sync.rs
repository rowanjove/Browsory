use std::path::PathBuf;
use tauri::State;
use tracing::info;

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
    let mut conn = db.conn.lock().unwrap();

    for source in sources {
        if !source.enabled {
            continue;
        }

        let history_path = PathBuf::from(&source.history_path);
        if !history_path.exists() {
            continue;
        }

        match sync_source_history(
            &mut conn,
            source.id,
            &source.browser,
            &source.profile,
            &history_path,
            source.last_visit_time,
            &db.temp_dir,
        ) {
            Ok(res) => results.push(res),
            Err(e) => {
                info!(
                    "Sync failed for source #{}: {}: {}",
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
