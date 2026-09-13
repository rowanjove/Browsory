pub mod dedup;
pub mod health;
pub mod normalize;
pub mod parser;
pub mod snapshot;
pub mod sync;
pub mod takeout;
pub mod watcher;

pub use dedup::calculate_event_hash;
pub use normalize::{normalize_url, NormalizedUrl};
pub use parser::{unix_millis_to_webkit_micros, webkit_micros_to_unix_millis};
pub use sync::sync_source_history;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_webkit_timestamp_conversion() {
        // 2024-01-01 00:00:00 UTC is 1704067200000 ms in Unix Epoch
        let unix_ms = 1704067200000;
        let webkit_micros = unix_millis_to_webkit_micros(unix_ms);
        let back_to_unix = webkit_micros_to_unix_millis(webkit_micros);
        assert_eq!(unix_ms, back_to_unix);
    }

    #[test]
    fn test_url_normalization() {
        let input = "HTTPS://GitHub.com:443/openai/whisper#readme";
        let norm = normalize_url(input);
        assert_eq!(norm.domain, "github.com");
        assert_eq!(norm.normalized_url, "https://github.com/openai/whisper");

        let local_input = "http://localhost:3000/dashboard?tab=1";
        let local_norm = normalize_url(local_input);
        assert_eq!(local_norm.domain, "localhost");
        assert_eq!(
            local_norm.normalized_url,
            "http://localhost:3000/dashboard?tab=1"
        );
    }

    #[test]
    fn test_event_hash_reproducibility() {
        let h1 = calculate_event_hash(1, 42, 1700000000000, "https://example.com");
        let h2 = calculate_event_hash(1, 42, 1700000000000, "https://example.com");
        let h3 = calculate_event_hash(2, 42, 1700000000000, "https://example.com");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_fts_search() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE urls (id INTEGER PRIMARY KEY, url TEXT, title TEXT, domain TEXT);
            CREATE VIRTUAL TABLE urls_fts USING fts5(title, url, domain, content='urls', content_rowid='id');
            INSERT INTO urls (id, url, title, domain) VALUES (1, 'https://example.com', '深入理解计算机系统', 'example.com');
            INSERT INTO urls_fts(rowid, title, url, domain) VALUES (1, '深入理解计算机系统', 'https://example.com', 'example.com');
            "#,
        ).unwrap();

        let query_kw = "计算机";
        let like_param = format!("%{}%", query_kw);
        let count_hybrid: i64 = conn.query_row(
            "SELECT count(*) FROM urls u WHERE u.id IN (SELECT rowid FROM urls_fts WHERE urls_fts MATCH '\"计算机\"*') OR u.title LIKE ?1 OR u.url LIKE ?1",
            rusqlite::params![like_param],
            |r| r.get(0),
        ).unwrap();
        assert_eq!(count_hybrid, 1);

        let query_short = "深入";
        let like_short = format!("%{}%", query_short);
        let count_short: i64 = conn.query_row(
            "SELECT count(*) FROM urls u WHERE u.id IN (SELECT rowid FROM urls_fts WHERE urls_fts MATCH '\"深入\"*') OR u.title LIKE ?1 OR u.url LIKE ?1",
            rusqlite::params![like_short],
            |r| r.get(0),
        ).unwrap();
        assert_eq!(count_short, 1);
    }
}
