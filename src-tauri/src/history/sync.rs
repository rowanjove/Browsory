use chrono::Utc;
use rusqlite::{params, Connection};
use std::path::Path;
use std::time::Instant;
use tracing::{error, info};

use super::dedup::calculate_event_hash;
use super::normalize::normalize_url;
use super::parser::read_chromium_history_snapshot;
use super::snapshot::{compute_source_db_signature, create_history_snapshot};
use crate::database::models::SyncResult;
use crate::database::repository::update_source_sync;
use crate::error::AppResult;

const OVERLAP_WINDOW_MILLIS: i64 = 48 * 60 * 60 * 1000; // 48 hours

pub fn sync_source_history(
    archive_conn: &mut Connection,
    source_id: i64,
    browser: &str,
    profile: &str,
    history_file_path: &Path,
    last_visit_time: i64,
    temp_dir: &Path,
) -> AppResult<SyncResult> {
    let start_time = Instant::now();
    let now = Utc::now().timestamp_millis();

    info!(
        "Starting sync for source #{}: {} [{}] at {:?}",
        source_id, browser, profile, history_file_path
    );

    // 1. Create temporary read-only snapshot
    let snapshot_guard = create_history_snapshot(history_file_path, temp_dir)?;

    // 2. Compute signature to detect if browser recreated the History database
    let current_signature = compute_source_db_signature(history_file_path);
    let prev_signature: Option<String> = archive_conn
        .query_row(
            "SELECT db_fingerprint FROM sources WHERE id = ?1",
            params![source_id],
            |r| r.get(0),
        )
        .ok()
        .flatten();

    let signature_changed = match &prev_signature {
        Some(prev) => {
            !prev.is_empty() && !current_signature.is_empty() && prev != &current_signature
        }
        None => false,
    };

    // 3. Determine since_unix_ms with 48h overlap window
    let since_unix_ms = if signature_changed {
        info!(
            "Source #{}: Browser History file signature changed (possible recreation/clear). Switching to safe rescan.",
            source_id
        );
        0
    } else if last_visit_time > OVERLAP_WINDOW_MILLIS {
        last_visit_time - OVERLAP_WINDOW_MILLIS
    } else {
        0
    };

    // 4. Parse raw records from snapshot
    let is_firefox = browser == "firefox"
        || history_file_path
            .to_string_lossy()
            .contains("places.sqlite");
    let records = if is_firefox {
        crate::browser::firefox::read_firefox_history_snapshot(
            &snapshot_guard.snapshot_path,
            since_unix_ms,
        )?
    } else {
        read_chromium_history_snapshot(&snapshot_guard.snapshot_path, since_unix_ms)?
    };
    let read_count = records.len() as u32;

    let mut inserted_count: u32 = 0;
    let mut duplicate_count: u32 = 0;
    let mut failed_count: u32 = 0;
    let mut max_visit_time = last_visit_time;
    let mut max_visit_id: i64 = 0;

    // 4. Batch import in transaction
    {
        let tx = archive_conn.transaction()?;

        let mut insert_url_stmt = tx.prepare_cached(
            r#"
            INSERT INTO urls (url, title, domain, normalized_url)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(url) DO UPDATE SET
                title = CASE WHEN length(?2) > 0 THEN ?2 ELSE title END
            RETURNING id;
            "#,
        )?;

        let mut insert_visit_stmt = tx.prepare_cached(
            r#"
            INSERT OR IGNORE INTO visits (
                source_id, url_id, source_visit_id, visit_time,
                transition, from_visit, visit_duration, event_hash, imported_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
        )?;

        let mut seen_hashes = std::collections::HashSet::new();

        for r in &records {
            let trimmed_url = r.url.trim();
            if trimmed_url.is_empty()
                || trimmed_url.starts_with("javascript:")
                || trimmed_url.starts_with("data:")
                || trimmed_url == "about:blank"
            {
                // Clean and ignore invalid / pseudo protocol URLs
                continue;
            }

            let cleaned_title = r.title.trim();
            let norm = normalize_url(trimmed_url);

            let event_hash = calculate_event_hash(
                source_id,
                r.source_visit_id,
                r.visit_time_unix_ms,
                &norm.normalized_url,
            );
            if !seen_hashes.insert(event_hash.clone()) {
                duplicate_count += 1;
                continue;
            }

            let url_id_res: Result<i64, _> = insert_url_stmt.query_row(
                params![
                    trimmed_url,
                    cleaned_title,
                    &norm.domain,
                    &norm.normalized_url
                ],
                |row| row.get(0),
            );

            let url_id = match url_id_res {
                Ok(id) => id,
                Err(e) => {
                    error!("Failed to insert or resolve url {}: {}", trimmed_url, e);
                    failed_count += 1;
                    continue;
                }
            };

            let changed = insert_visit_stmt.execute(params![
                source_id,
                url_id,
                r.source_visit_id,
                r.visit_time_unix_ms,
                r.transition,
                r.from_visit,
                r.visit_duration,
                &event_hash,
                now,
            ])?;

            if changed > 0 {
                inserted_count += 1;
                if r.visit_time_unix_ms > max_visit_time {
                    max_visit_time = r.visit_time_unix_ms;
                }
                if r.source_visit_id > max_visit_id {
                    max_visit_id = r.source_visit_id;
                }
            } else {
                duplicate_count += 1;
            }
        }

        drop(insert_url_stmt);
        drop(insert_visit_stmt);

        // Record job in database
        tx.execute(
            r#"
            INSERT INTO import_jobs (
                source_id, started_at, finished_at, read_count,
                inserted_count, duplicate_count, failed_count, status
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'success')
            "#,
            params![
                source_id,
                now,
                Utc::now().timestamp_millis(),
                read_count,
                inserted_count,
                duplicate_count,
                failed_count,
            ],
        )?;

        tx.commit()?;
    }

    // 5. Update source metadata
    update_source_sync(
        archive_conn,
        source_id,
        max_visit_id,
        max_visit_time,
        now,
        Some(&current_signature),
    )?;

    let duration_ms = start_time.elapsed().as_millis() as u64;

    info!(
        "Sync completed for source #{}: read={}, inserted={}, duplicate={}, failed={}, took {}ms",
        source_id, read_count, inserted_count, duplicate_count, failed_count, duration_ms
    );

    Ok(SyncResult {
        source_id,
        browser: browser.to_string(),
        profile: profile.to_string(),
        read_count,
        inserted_count,
        duplicate_count,
        failed_count,
        duration_ms,
    })
}
