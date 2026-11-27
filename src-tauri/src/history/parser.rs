use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
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
    unix_millis
        .checked_mul(1000)
        .and_then(|micros| micros.checked_add(WEBKIT_EPOCH_OFFSET_MICROS))
        .unwrap_or(i64::MAX)
}

#[derive(Clone, zeroize::Zeroize, zeroize::ZeroizeOnDrop)]
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
    conn: &Connection,
    since_unix_ms: i64,
) -> AppResult<Vec<RawVisitRecord>> {
    let mut records = Vec::new();
    let mut bytes = 0usize;
    // A SQL sorter could expand repeated titles before the Vec budget applies.
    for_each_chromium_record(conn, since_unix_ms, false, |record| {
        push_buffered_record(&mut records, &mut bytes, record)
    })?;
    info!(
        "Parsed {} raw visit records from private snapshot",
        records.len()
    );
    Ok(records)
}

pub(crate) fn push_buffered_record(
    records: &mut Vec<RawVisitRecord>,
    bytes: &mut usize,
    record: RawVisitRecord,
) -> AppResult<()> {
    const MAX_BYTES: usize = 64 * 1024 * 1024;
    const MAX_RECORDS: usize = 100_000;
    let size = record
        .url
        .len()
        .checked_add(record.title.len())
        .and_then(|size| size.checked_add(std::mem::size_of::<RawVisitRecord>()))
        .and_then(|size| bytes.checked_add(size))
        .ok_or(crate::error::AppError::MemoryPreparationLimit)?;
    if size > MAX_BYTES || records.len() >= MAX_RECORDS {
        return Err(crate::error::AppError::MemoryPreparationLimit);
    }
    *bytes = size;
    records.push(record);
    Ok(())
}

fn utf8_bounded_prefix(slice: &[u8], max_bytes: usize) -> String {
    let capped = &slice[..slice.len().min(max_bytes)];
    match std::str::from_utf8(capped) {
        Ok(valid) => valid.to_string(),
        Err(err) => {
            if err.error_len().is_none() {
                // Cut landed mid-codepoint at the end; prefix up to valid_up_to is valid UTF-8.
                String::from_utf8(capped[..err.valid_up_to()].to_vec()).unwrap_or_default()
            } else {
                let mut lossy = String::from_utf8_lossy(capped).into_owned();
                while lossy.len() > max_bytes {
                    lossy.pop();
                }
                lossy
            }
        }
    }
}

pub(crate) fn limited_text(
    row: &rusqlite::Row<'_>,
    index: usize,
    limit: usize,
) -> rusqlite::Result<String> {
    let value = row.get_ref(index)?;
    if let rusqlite::types::ValueRef::Text(bytes) = value {
        if bytes.len() > limit {
            const FINGERPRINT_TAG_LEN: usize = 27; // "...[sha256:0123456789abcdef]"
            if limit <= FINGERPRINT_TAG_LEN {
                return Ok(utf8_bounded_prefix(bytes, limit));
            }
            let prefix = utf8_bounded_prefix(bytes, limit - FINGERPRINT_TAG_LEN);
            let digest = Sha256::digest(bytes);
            let tag = format!(
                "{:016x}",
                u64::from_be_bytes(digest[0..8].try_into().unwrap())
            );
            let bounded = format!("{prefix}...[sha256:{tag}]");
            debug!(
                index,
                original_len = bytes.len(),
                bounded_len = bounded.len(),
                "Truncated oversized browser history text field cleanly"
            );
            return Ok(bounded);
        }
    }
    row.get(index)
}

pub(crate) fn for_each_chromium_record<F>(
    conn: &Connection,
    since_unix_ms: i64,
    ordered: bool,
    mut consume: F,
) -> AppResult<()>
where
    F: FnMut(RawVisitRecord) -> AppResult<()>,
{
    let since = unix_millis_to_webkit_micros(since_unix_ms);
    debug!("Reading history since unix_ms: {}", since_unix_ms);
    // The spool scan does not sort, avoiding an unbounded in-memory sorter.
    let order = if ordered {
        " ORDER BY v.visit_time ASC"
    } else {
        ""
    };
    let sql = format!("SELECT v.id, v.visit_time, COALESCE(v.from_visit,0), COALESCE(v.transition,0),
        COALESCE(v.visit_duration,0),COALESCE(u.url,''),COALESCE(u.title,'') FROM visits v JOIN urls u ON v.url=u.id
        WHERE v.visit_time>=?1{order}");
    let mut statement = conn.prepare(&sql)?;
    let mut rows = statement.query(params![since])?;
    while let Some(row) = rows.next()? {
        consume(RawVisitRecord {
            source_visit_id: row.get(0)?,
            visit_time_unix_ms: webkit_micros_to_unix_millis(row.get(1)?),
            from_visit: row.get(2)?,
            transition: row.get(3)?,
            visit_duration: row.get(4)?,
            url: limited_text(row, 5, 64 * 1024)?,
            title: limited_text(row, 6, 256 * 1024)?,
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_conversion_is_saturating_at_i64_boundaries() {
        assert_eq!(webkit_micros_to_unix_millis(0), 0);
        assert_eq!(unix_millis_to_webkit_micros(0), 0);
        assert_eq!(unix_millis_to_webkit_micros(-1), 0);
        assert_eq!(unix_millis_to_webkit_micros(i64::MAX), i64::MAX);
    }

    #[test]
    fn oversized_chromium_urls_and_multibyte_titles_are_bounded_distinctly() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE urls(id INTEGER PRIMARY KEY, url TEXT, title TEXT);
             CREATE TABLE visits(id INTEGER PRIMARY KEY, url INTEGER, visit_time INTEGER, from_visit INTEGER, transition INTEGER, visit_duration INTEGER);",
        )
        .unwrap();

        let common_prefix = format!("https://example.com/search?q={}", "中".repeat(25_000));
        let url_a = format!("{common_prefix}&tail=A");
        let url_b = format!("{common_prefix}&tail=B");
        assert!(url_a.len() > 64 * 1024);
        let data_url = format!("data:image/png;base64,{}", "A".repeat(120 * 1024));
        let huge_title = "标".repeat(100_000);

        conn.execute(
            "INSERT INTO urls VALUES (1, ?1, ?2)",
            params![url_a, huge_title],
        )
        .unwrap();
        conn.execute("INSERT INTO urls VALUES (2, ?1, 'Second')", params![url_b])
            .unwrap();
        conn.execute(
            "INSERT INTO urls VALUES (3, ?1, 'Inline Data')",
            params![data_url],
        )
        .unwrap();
        conn.execute_batch(
            "INSERT INTO visits VALUES (10, 1, 13380000000000000, 0, 0, 1000);
             INSERT INTO visits VALUES (11, 2, 13380000001000000, 0, 0, 1000);
             INSERT INTO visits VALUES (12, 3, 13380000002000000, 0, 0, 1000);",
        )
        .unwrap();

        let records = read_chromium_history_snapshot(&conn, 0).unwrap();
        assert_eq!(records.len(), 3);
        assert!(records[0].url.len() <= 64 * 1024);
        assert!(records[0].title.len() <= 256 * 1024);
        assert!(records[1].url.len() <= 64 * 1024);
        assert_ne!(records[0].url, records[1].url);
        assert!(records[0].url.starts_with("https://example.com/"));
        assert!(records[2].url.starts_with("data:"));
    }
}
