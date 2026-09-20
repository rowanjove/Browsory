use rusqlite::{params, Connection, OptionalExtension};

use crate::database::models::Source;
use crate::error::{AppError, AppResult};

pub fn list_sources(conn: &Connection) -> AppResult<Vec<Source>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, browser, profile, source_type, history_path, db_fingerprint,
               last_visit_id, last_visit_time, last_sync_at, enabled
        FROM sources
        ORDER BY browser ASC, profile ASC
        "#,
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(Source {
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
    })?;

    let mut sources = Vec::new();
    for r in rows {
        sources.push(r?);
    }
    Ok(sources)
}

pub fn get_or_create_source(
    conn: &Connection,
    browser: &str,
    profile: &str,
    source_type: &str,
    history_path: &str,
) -> AppResult<Source> {
    let existing: Option<Source> = conn
        .query_row(
            r#"
            SELECT id, browser, profile, source_type, history_path, db_fingerprint,
                   last_visit_id, last_visit_time, last_sync_at, enabled
            FROM sources
            WHERE browser = ?1 AND profile = ?2 AND history_path = ?3
            "#,
            params![browser, profile, history_path],
            |row| {
                Ok(Source {
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
            },
        )
        .optional()?;

    if let Some(src) = existing {
        return Ok(src);
    }

    conn.execute(
        r#"
        INSERT INTO sources (browser, profile, source_type, history_path)
        VALUES (?1, ?2, ?3, ?4)
        "#,
        params![browser, profile, source_type, history_path],
    )?;

    let id = conn.last_insert_rowid();
    Ok(Source {
        id,
        browser: browser.to_string(),
        profile: profile.to_string(),
        source_type: source_type.to_string(),
        history_path: history_path.to_string(),
        db_fingerprint: None,
        last_visit_id: 0,
        last_visit_time: 0,
        last_sync_at: None,
        enabled: true,
    })
}

pub fn get_source_by_id(conn: &Connection, source_id: i64) -> AppResult<Option<Source>> {
    conn.query_row(
        r#"
        SELECT id, browser, profile, source_type, history_path, db_fingerprint,
               last_visit_id, last_visit_time, last_sync_at, enabled
        FROM sources
        WHERE id = ?1
        "#,
        params![source_id],
        |row| {
            Ok(Source {
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
        },
    )
    .optional()
    .map_err(AppError::from)
}

pub fn update_source_sync(
    conn: &Connection,
    source_id: i64,
    last_visit_id: i64,
    last_visit_time: i64,
    last_sync_at: i64,
    db_fingerprint: Option<&str>,
) -> AppResult<()> {
    conn.execute(
        r#"
        UPDATE sources
        SET last_visit_id = MAX(last_visit_id, ?1),
            last_visit_time = MAX(last_visit_time, ?2),
            last_sync_at = ?3,
            db_fingerprint = COALESCE(?5, db_fingerprint)
        WHERE id = ?4
        "#,
        params![
            last_visit_id,
            last_visit_time,
            last_sync_at,
            source_id,
            db_fingerprint
        ],
    )?;
    Ok(())
}
