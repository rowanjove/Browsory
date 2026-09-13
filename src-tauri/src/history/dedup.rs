use sha2::{Digest, Sha256};

pub fn calculate_event_hash(
    source_id: i64,
    source_visit_id: i64,
    visit_time: i64,
    url: &str,
) -> String {
    let mut hasher = Sha256::new();
    let payload = format!(
        "{}:{}:{}:{}",
        source_id,
        source_visit_id,
        visit_time,
        url.trim()
    );
    hasher.update(payload.as_bytes());
    format!("{:x}", hasher.finalize())
}
