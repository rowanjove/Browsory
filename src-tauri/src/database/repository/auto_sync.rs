use rusqlite::{params, Connection};
use uuid::Uuid;

use super::{get_setting, set_setting};
use crate::database::models::{AutoSyncConfig, AutoSyncRunItem, AutoSyncSourceSummary};
use crate::error::{AppError, AppResult};

pub const MIN_AUTO_SYNC_INTERVAL_DAYS: u32 = 1;
pub const MAX_AUTO_SYNC_INTERVAL_DAYS: u32 = 365;
pub const DEFAULT_AUTO_SYNC_INTERVAL_DAYS: u32 = 30;
pub const MAX_STORED_AUTO_SYNC_RUNS: u32 = 100;

pub const KEY_AUTO_SYNC_ENABLED: &str = "auto_sync_enabled";
pub const KEY_AUTO_SYNC_INTERVAL_DAYS: &str = "auto_sync_interval_days";
pub const KEY_AUTO_SYNC_LAST_RUN_AT: &str = "auto_sync_last_run_at";
pub const KEY_AUTO_SYNC_LAST_SUCCESS_AT: &str = "auto_sync_last_success_at";
pub const KEY_AUTO_SYNC_NEXT_RUN_AT: &str = "auto_sync_next_run_at";
pub const KEY_AUTO_SYNC_LAST_EXE_PATH: &str = "auto_sync_last_exe_path";
pub const KEY_AUTO_SYNC_INSTANCE_ID: &str = "auto_sync_instance_id";

pub fn validate_interval_days(days: u32) -> AppResult<u32> {
    if !(MIN_AUTO_SYNC_INTERVAL_DAYS..=MAX_AUTO_SYNC_INTERVAL_DAYS).contains(&days) {
        return Err(AppError::Other(format!(
            "同步周期必须在 {} 到 {} 天之间",
            MIN_AUTO_SYNC_INTERVAL_DAYS, MAX_AUTO_SYNC_INTERVAL_DAYS
        )));
    }
    Ok(days)
}

fn parse_optional_i64(raw: Option<String>) -> Option<i64> {
    raw.and_then(|s| {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else {
            trimmed.parse::<i64>().ok()
        }
    })
}

pub fn get_auto_sync_config(conn: &Connection) -> AppResult<AutoSyncConfig> {
    let enabled = get_setting(conn, KEY_AUTO_SYNC_ENABLED)?.is_some_and(|v| v.trim() == "1");
    let interval_days = get_setting(conn, KEY_AUTO_SYNC_INTERVAL_DAYS)?
        .and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|d| (MIN_AUTO_SYNC_INTERVAL_DAYS..=MAX_AUTO_SYNC_INTERVAL_DAYS).contains(d))
        .unwrap_or(DEFAULT_AUTO_SYNC_INTERVAL_DAYS);

    let last_run_at = parse_optional_i64(get_setting(conn, KEY_AUTO_SYNC_LAST_RUN_AT)?);
    let last_success_at = parse_optional_i64(get_setting(conn, KEY_AUTO_SYNC_LAST_SUCCESS_AT)?);
    let next_run_at = parse_optional_i64(get_setting(conn, KEY_AUTO_SYNC_NEXT_RUN_AT)?);

    Ok(AutoSyncConfig {
        enabled,
        interval_days,
        last_run_at,
        last_success_at,
        next_run_at,
    })
}

pub fn set_auto_sync_config(
    conn: &Connection,
    enabled: bool,
    interval_days: u32,
    next_run_at: Option<i64>,
) -> AppResult<()> {
    validate_interval_days(interval_days)?;
    set_setting(conn, KEY_AUTO_SYNC_ENABLED, if enabled { "1" } else { "0" })?;
    set_setting(
        conn,
        KEY_AUTO_SYNC_INTERVAL_DAYS,
        &interval_days.to_string(),
    )?;
    let next_str = next_run_at.map(|v| v.to_string()).unwrap_or_default();
    set_setting(conn, KEY_AUTO_SYNC_NEXT_RUN_AT, &next_str)?;
    Ok(())
}

pub fn update_auto_sync_timestamps(
    conn: &Connection,
    last_run_at: Option<i64>,
    last_success_at: Option<i64>,
    next_run_at: Option<i64>,
    last_exe_path: Option<&str>,
) -> AppResult<()> {
    if let Some(ts) = last_run_at {
        set_setting(conn, KEY_AUTO_SYNC_LAST_RUN_AT, &ts.to_string())?;
    }
    if let Some(ts) = last_success_at {
        set_setting(conn, KEY_AUTO_SYNC_LAST_SUCCESS_AT, &ts.to_string())?;
    }
    if let Some(ts) = next_run_at {
        set_setting(conn, KEY_AUTO_SYNC_NEXT_RUN_AT, &ts.to_string())?;
    }
    if let Some(path) = last_exe_path {
        set_setting(conn, KEY_AUTO_SYNC_LAST_EXE_PATH, path)?;
    }
    Ok(())
}

pub fn get_or_create_auto_sync_instance_id(conn: &Connection) -> AppResult<String> {
    if let Some(existing) = get_setting(conn, KEY_AUTO_SYNC_INSTANCE_ID)? {
        let trimmed = existing.trim().to_string();
        if !trimmed.is_empty() {
            return Ok(trimmed);
        }
    }
    let id = Uuid::new_v4().simple().to_string();
    set_setting(conn, KEY_AUTO_SYNC_INSTANCE_ID, &id)?;
    Ok(id)
}

pub fn count_bound_enabled_sources(conn: &Connection) -> AppResult<u32> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sources WHERE enabled = 1 AND source_type != 'takeout'",
        [],
        |r| r.get(0),
    )?;
    Ok(count.max(0) as u32)
}

pub fn create_auto_sync_run(
    conn: &Connection,
    run_uuid: &str,
    trigger_type: &str,
    started_at: i64,
    sources_total: u32,
) -> AppResult<i64> {
    conn.execute(
        r#"
        INSERT INTO auto_sync_runs (
            run_uuid, trigger_type, started_at, status, sources_total
        ) VALUES (?1, ?2, ?3, 'running', ?4)
        "#,
        params![run_uuid, trigger_type, started_at, sources_total as i64],
    )?;
    Ok(conn.last_insert_rowid())
}

#[allow(clippy::too_many_arguments)]
pub fn finish_auto_sync_run(
    conn: &Connection,
    run_id: i64,
    finished_at: i64,
    status: &str,
    sources_total: u32,
    sources_succeeded: u32,
    sources_skipped: u32,
    sources_failed: u32,
    total_read: u32,
    total_inserted: u32,
    total_duplicate: u32,
    duration_ms: u64,
    error_summary: Option<&str>,
    details: &[AutoSyncSourceSummary],
) -> AppResult<()> {
    let details_json = serde_json::to_string(details)?;
    conn.execute(
        r#"
        UPDATE auto_sync_runs
        SET finished_at = ?2,
            status = ?3,
            sources_total = ?4,
            sources_succeeded = ?5,
            sources_skipped = ?6,
            sources_failed = ?7,
            total_read = ?8,
            total_inserted = ?9,
            total_duplicate = ?10,
            duration_ms = ?11,
            error_summary = ?12,
            details_json = ?13
        WHERE id = ?1
        "#,
        params![
            run_id,
            finished_at,
            status,
            sources_total as i64,
            sources_succeeded as i64,
            sources_skipped as i64,
            sources_failed as i64,
            total_read as i64,
            total_inserted as i64,
            total_duplicate as i64,
            duration_ms as i64,
            error_summary,
            details_json,
        ],
    )?;
    Ok(())
}

pub fn list_auto_sync_runs(conn: &Connection, limit: u32) -> AppResult<Vec<AutoSyncRunItem>> {
    let capped = limit.clamp(1, 50) as i64;
    let mut stmt = conn.prepare(
        r#"
        SELECT id, run_uuid, trigger_type, started_at, finished_at, status,
               sources_total, sources_succeeded, sources_skipped, sources_failed,
               total_read, total_inserted, total_duplicate, duration_ms,
               error_summary, details_json
        FROM auto_sync_runs
        ORDER BY started_at DESC, id DESC
        LIMIT ?1
        "#,
    )?;

    let rows = stmt.query_map([capped], |row| {
        let details_raw: Option<String> = row.get(15)?;
        let details: Vec<AutoSyncSourceSummary> = details_raw
            .as_deref()
            .and_then(|json| serde_json::from_str(json).ok())
            .unwrap_or_default();

        Ok(AutoSyncRunItem {
            id: row.get(0)?,
            run_uuid: row.get(1)?,
            trigger_type: row.get(2)?,
            started_at: row.get(3)?,
            finished_at: row.get(4)?,
            status: row.get(5)?,
            sources_total: row.get::<_, i64>(6)?.max(0) as u32,
            sources_succeeded: row.get::<_, i64>(7)?.max(0) as u32,
            sources_skipped: row.get::<_, i64>(8)?.max(0) as u32,
            sources_failed: row.get::<_, i64>(9)?.max(0) as u32,
            total_read: row.get::<_, i64>(10)?.max(0) as u32,
            total_inserted: row.get::<_, i64>(11)?.max(0) as u32,
            total_duplicate: row.get::<_, i64>(12)?.max(0) as u32,
            duration_ms: row.get::<_, i64>(13)?.max(0) as u64,
            error_summary: row.get(14)?,
            details,
        })
    })?;

    let mut runs = Vec::new();
    for r in rows {
        runs.push(r?);
    }
    Ok(runs)
}

pub fn mark_stale_running_runs(conn: &Connection, now_ms: i64) -> AppResult<usize> {
    let changed = conn.execute(
        r#"
        UPDATE auto_sync_runs
        SET status = 'failed',
            finished_at = COALESCE(finished_at, ?1),
            error_summary = COALESCE(error_summary, '后台同步任务异常中断或超时终止')
        WHERE status = 'running'
        "#,
        params![now_ms],
    )?;
    Ok(changed)
}

pub fn prune_old_auto_sync_runs(conn: &Connection, keep: u32) -> AppResult<usize> {
    let deleted = conn.execute(
        r#"
        DELETE FROM auto_sync_runs
        WHERE id NOT IN (
            SELECT id FROM auto_sync_runs ORDER BY started_at DESC, id DESC LIMIT ?1
        )
        "#,
        params![keep.max(1) as i64],
    )?;
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::migrations::run_migrations;

    #[test]
    fn interval_validation_enforces_1_to_365_days() {
        assert!(validate_interval_days(0).is_err());
        assert_eq!(validate_interval_days(1).unwrap(), 1);
        assert_eq!(validate_interval_days(30).unwrap(), 30);
        assert_eq!(validate_interval_days(365).unwrap(), 365);
        assert!(validate_interval_days(366).is_err());
    }

    #[test]
    fn migration_v16_defaults_and_run_lifecycle() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();

        let cfg = get_auto_sync_config(&conn).unwrap();
        assert!(!cfg.enabled);
        assert_eq!(cfg.interval_days, 30);
        assert!(cfg.last_run_at.is_none());
        assert!(cfg.next_run_at.is_none());

        set_auto_sync_config(&conn, true, 7, Some(1_700_000_000_000)).unwrap();
        let updated = get_auto_sync_config(&conn).unwrap();
        assert!(updated.enabled);
        assert_eq!(updated.interval_days, 7);
        assert_eq!(updated.next_run_at, Some(1_700_000_000_000));

        let run_id = create_auto_sync_run(&conn, "run-1", "scheduled", 1000, 1).unwrap();
        finish_auto_sync_run(
            &conn,
            run_id,
            2000,
            "success",
            2,
            1,
            1,
            0,
            10,
            5,
            5,
            1000,
            None,
            &[],
        )
        .unwrap();

        let runs = list_auto_sync_runs(&conn, 10).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, "success");
        assert_eq!(runs[0].sources_total, 2);
        assert_eq!(
            runs[0].sources_total,
            runs[0].sources_succeeded + runs[0].sources_skipped + runs[0].sources_failed
        );
        assert_eq!(runs[0].total_inserted, 5);

        let stale_id = create_auto_sync_run(&conn, "run-2", "scheduled", 3000, 1).unwrap();
        let fixed = mark_stale_running_runs(&conn, 4000).unwrap();
        assert_eq!(fixed, 1);
        let runs_after = list_auto_sync_runs(&conn, 10).unwrap();
        assert_eq!(runs_after[0].id, stale_id);
        assert_eq!(runs_after[0].status, "failed");
    }
}
