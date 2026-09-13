use rusqlite::{params, Connection, OpenFlags};
use std::path::Path;
use tracing::{debug, info};

use crate::error::AppResult;

pub const WEBKIT_EPOCH_OFFSET_MICROS: i64 = 11_644_473_600_000_000;

pub fn webkit_micros_to_unix_millis(webkit_micros: i64) -> i64 {
    if webkit_micros <= WEBKIT_EPOCH_OFFSET_MICROS {
        return 0;
    }
    (webkit_micros - WEBKIT_EPOCH_OFFSET_MICROS) / 1000
}

pub fn unix_millis_to_webkit_micros(unix_millis: i64) -> i64 {
    if unix_millis <= 0 {
        return 0;
    }
    (unix_millis * 1000) + WEBKIT_EPOCH_OFFSET_MICROS
}

#[derive(Debug, Clone)]
pub struct RawVisitRecord {
    pub source_visit_id: i64,
    pub visit_time_unix_ms: i64,
    pub from_visit: i64,
    pub transition: i64,
    pub visit_duration: i64,
    pub url: String,
    pub title: String,
}

pub fn read_chromium_history_snapshot(
    snapshot_path: &Path,
    since_unix_ms: i64,
) -> AppResult<Vec<RawVisitRecord>> {
    let conn = Connection::open_with_flags(
        snapshot_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )?;

    // Safety checks
    let _: String = conn.query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?;

    let since_webkit_micros = unix_millis_to_webkit_micros(since_unix_ms);
    debug!(
        "Reading history since unix_ms: {}, webkit_micros: {}",
        since_unix_ms, since_webkit_micros
    );

    let mut stmt = conn.prepare(
        r#"
        SELECT v.id, v.visit_time, COALESCE(v.from_visit, 0), COALESCE(v.transition, 0),
               COALESCE(v.visit_duration, 0), u.url, COALESCE(u.title, '')
        FROM visits v
        JOIN urls u ON v.url = u.id
        WHERE v.visit_time >= ?1
        ORDER BY v.visit_time ASC
        "#,
    )?;

    let rows = stmt.query_map(params![since_webkit_micros], |row| {
        let source_visit_id: i64 = row.get(0)?;
        let webkit_time: i64 = row.get(1)?;
        let from_visit: i64 = row.get(2)?;
        let transition: i64 = row.get(3)?;
        let visit_duration: i64 = row.get(4)?;
        let url: String = row.get(5)?;
        let title: String = row.get(6)?;

        Ok(RawVisitRecord {
            source_visit_id,
            visit_time_unix_ms: webkit_micros_to_unix_millis(webkit_time),
            from_visit,
            transition,
            visit_duration,
            url,
            title,
        })
    })?;

    let mut records = Vec::new();
    for r in rows {
        records.push(r?);
    }

    info!(
        "Parsed {} raw visit records from snapshot: {:?}",
        records.len(),
        snapshot_path
    );

    Ok(records)
}
