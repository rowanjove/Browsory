use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::de::{self, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tracing::info;

use crate::database::models::TakeoutImportSummary;
use crate::error::{AppError, AppResult};
use crate::history::dedup::calculate_event_hash;
use crate::history::normalize::normalize_url;

pub const TAKEOUT_MAX_FILE_BYTES: u64 = 128 * 1024 * 1024;
pub const TAKEOUT_MAX_RECORDS: usize = 100_000;
pub const TAKEOUT_MAX_FIELD_BYTES: usize = 256 * 1024;
pub const TAKEOUT_MAX_EXPANDED_BYTES: usize = 64 * 1024 * 1024;

pub type TakeoutCancelProbe = Arc<dyn Fn() -> bool + Send + Sync>;

struct CancellationReader<R> {
    inner: R,
    cancelled: TakeoutCancelProbe,
    remaining: u64,
}

impl<R> CancellationReader<R> {
    fn new(inner: R, cancelled: TakeoutCancelProbe) -> Self {
        Self {
            inner,
            cancelled,
            remaining: TAKEOUT_MAX_FILE_BYTES,
        }
    }
}

impl<R: Read> Read for CancellationReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if (self.cancelled)() {
            return Err(io::Error::other("Google Takeout parse cancelled"));
        }
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            let mut extra = [0u8; 1];
            return if self.inner.read(&mut extra)? == 0 {
                Ok(0)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Google Takeout exceeds the 128 MiB read limit",
                ))
            };
        }
        let len = buffer.len().min(self.remaining as usize);
        let read = self.inner.read(&mut buffer[..len])?;
        self.remaining -= read as u64;
        Ok(read)
    }
}

fn deserialize_bounded_optional_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    if value
        .as_ref()
        .is_some_and(|text| text.len() > TAKEOUT_MAX_FIELD_BYTES)
    {
        return Err(de::Error::custom("Google Takeout 字段超过长度限制"));
    }
    Ok(value)
}

fn deserialize_bounded_optional_value<'de, D>(
    deserializer: D,
) -> Result<Option<serde_json::Value>, D::Error>
where
    D: Deserializer<'de>,
{
    struct Scalar;
    impl<'de> Visitor<'de> for Scalar {
        type Value = Option<serde_json::Value>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a bounded timestamp string or number")
        }
        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
            Ok(Some(value.into()))
        }
        fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
            Ok(Some(value.into()))
        }
        fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
            Ok(serde_json::Number::from_f64(value).map(serde_json::Value::Number))
        }
        fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
            if value.len() > TAKEOUT_MAX_FIELD_BYTES {
                return Err(E::custom("Google Takeout 时间字段超过长度限制"));
            }
            Ok(Some(value.into()))
        }
    }
    // Objects/arrays are rejected before building an arbitrarily large Value.
    deserializer.deserialize_any(Scalar)
}

#[derive(Debug, Clone, Deserialize)]
struct TakeoutRecordItem {
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_bounded_optional_string")]
    pub url: Option<String>,
    #[serde(rename = "titleUrl")]
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_bounded_optional_string")]
    pub title_url: Option<String>,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_bounded_optional_string")]
    pub virtual_url: Option<String>,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_bounded_optional_string")]
    pub title: Option<String>,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_bounded_optional_string")]
    pub header: Option<String>,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_bounded_optional_value")]
    pub time_usec: Option<serde_json::Value>,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_bounded_optional_value")]
    pub timestamp_usec: Option<serde_json::Value>,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_bounded_optional_value")]
    pub timestamp_msec: Option<serde_json::Value>,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_bounded_optional_string")]
    pub time: Option<String>,
}

#[derive(Debug)]
struct LimitedRecords(Vec<TakeoutRecordItem>);

impl<'de> Deserialize<'de> for LimitedRecords {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct RecordsVisitor;

        impl<'de> Visitor<'de> for RecordsVisitor {
            type Value = LimitedRecords;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a bounded array of Google Takeout records")
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut records = Vec::new();
                let mut expanded = 0usize;
                while let Some(record) = sequence.next_element::<TakeoutRecordItem>()? {
                    if records.len() >= TAKEOUT_MAX_RECORDS {
                        return Err(de::Error::custom("Google Takeout 记录数量超过限制"));
                    }
                    expanded = expanded.saturating_add(expanded_item_bytes(&record));
                    if expanded > TAKEOUT_MAX_EXPANDED_BYTES {
                        return Err(de::Error::custom(
                            "Google Takeout records exceed the 64 MiB preparation limit",
                        ));
                    }
                    records.push(record);
                }
                Ok(LimitedRecords(records))
            }
        }

        deserializer.deserialize_seq(RecordsVisitor)
    }
}

// Unknown JSON fields are consumed with IgnoredAny. Do not build a whole
// serde_json::Value DOM merely to discover a legacy wrapper's record array.
macro_rules! ignore_scalar {
    () => {
        fn visit_bool<E: de::Error>(self, _: bool) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_i64<E: de::Error>(self, _: i64) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_u64<E: de::Error>(self, _: u64) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_f64<E: de::Error>(self, _: f64) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_str<E: de::Error>(self, _: &str) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
    };
}

struct OptionalRecord(Option<TakeoutRecordItem>);
impl<'de> Deserialize<'de> for OptionalRecord {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RecordVisitor;
        impl<'de> Visitor<'de> for RecordVisitor {
            type Value = Option<TakeoutRecordItem>;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a history record or unrelated JSON value")
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
                TakeoutRecordItem::deserialize(de::value::MapAccessDeserializer::new(map)).map(Some)
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(None)
            }
            ignore_scalar!();
        }
        deserializer
            .deserialize_any(RecordVisitor)
            .map(OptionalRecord)
    }
}

struct OptionalArray<const KNOWN: bool>(Option<LimitedRecords>);
impl<'de, const KNOWN: bool> Deserialize<'de> for OptionalArray<KNOWN> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ArrayVisitor<const K: bool>;
        impl<'de, const K: bool> Visitor<'de> for ArrayVisitor<K> {
            type Value = Option<LimitedRecords>;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a bounded history array")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                if K {
                    return LimitedRecords::deserialize(de::value::SeqAccessDeserializer::new(seq))
                        .map(Some);
                }
                let mut records = Vec::new();
                let mut expanded = 0usize;
                let mut seen = 0usize;
                while let Some(OptionalRecord(record)) = seq.next_element()? {
                    seen += 1;
                    if seen > TAKEOUT_MAX_RECORDS {
                        return Err(de::Error::custom(
                            "Google Takeout record array exceeds 100000 items",
                        ));
                    }
                    if let Some(record) = record {
                        expanded = expanded.saturating_add(expanded_item_bytes(&record));
                        if expanded > TAKEOUT_MAX_EXPANDED_BYTES {
                            return Err(de::Error::custom(
                                "Google Takeout records exceed the 64 MiB preparation limit",
                            ));
                        }
                        records.push(record);
                    }
                }
                Ok((!records.is_empty()).then_some(LimitedRecords(records)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                Ok(None)
            }
            ignore_scalar!();
        }
        deserializer
            .deserialize_any(ArrayVisitor::<KNOWN>)
            .map(OptionalArray)
    }
}

struct TakeoutDocument<const KNOWN: bool>(Option<LimitedRecords>);
impl<'de, const KNOWN: bool> Deserialize<'de> for TakeoutDocument<KNOWN> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct DocumentVisitor<const K: bool>;
        impl<'de, const K: bool> Visitor<'de> for DocumentVisitor<K> {
            type Value = Option<LimitedRecords>;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a Google Takeout history object or array")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
                LimitedRecords::deserialize(de::value::SeqAccessDeserializer::new(seq)).map(Some)
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                const KEYS: &[&str] = &[
                    "Browser History",
                    "浏览器历史记录",
                    "历史记录",
                    "瀏覽器歷史記錄",
                    "歷史記錄",
                    "records",
                    "Records",
                    "history",
                    "BrowserHistory",
                ];
                let mut chosen = None;
                let mut rank = usize::MAX;
                while let Some(key) = map.next_key::<String>()? {
                    let candidate_rank = KEYS.iter().position(|candidate| *candidate == key);
                    if (K && candidate_rank.is_none())
                        || (chosen.is_some()
                            && (!K || candidate_rank.unwrap_or(usize::MAX) >= rank))
                    {
                        map.next_value::<IgnoredAny>()?;
                        continue;
                    }
                    if let Some(records) = map.next_value::<OptionalArray<K>>()?.0 {
                        if !records.0.is_empty() {
                            chosen = Some(records);
                            rank = candidate_rank.unwrap_or(usize::MAX);
                        }
                    }
                }
                Ok(chosen)
            }
        }
        deserializer
            .deserialize_any(DocumentVisitor::<KNOWN>)
            .map(TakeoutDocument)
    }
}

#[derive(Debug)]
pub struct PreparedTakeoutData {
    file_path: PathBuf,
    records: Vec<TakeoutRecordItem>,
    started_at: i64,
}

fn parse_json_i64(val: &Option<serde_json::Value>) -> Option<i64> {
    let value = val.as_ref()?;
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
        .or_else(|| {
            value
                .as_str()
                .and_then(|text| text.trim().parse::<i64>().ok())
        })
}

fn parse_json_timestamp(val: &Option<serde_json::Value>, is_msec: bool) -> Option<i64> {
    let value = parse_json_i64(val)?;
    if is_msec {
        Some(value)
    } else {
        Some(value / 1000)
    }
}

fn parse_iso_time(time_str: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(time_str)
        .or_else(|_| chrono::DateTime::parse_from_str(time_str, "%Y-%m-%d %H:%M:%S %z"))
        .map(|dt| dt.timestamp_millis())
        .ok()
}

fn check_cancelled(cancelled: &TakeoutCancelProbe, phase: &str) -> AppResult<()> {
    if cancelled() {
        return Err(AppError::VaultLocked(format!(
            "Google Takeout {}已取消",
            phase
        )));
    }
    Ok(())
}

fn first_non_empty_url(item: &TakeoutRecordItem) -> Option<String> {
    [&item.url, &item.title_url, &item.virtual_url]
        .into_iter()
        .filter_map(|candidate| candidate.as_deref())
        .map(str::trim)
        .find(|candidate| !candidate.is_empty())
        .map(str::to_owned)
}

fn stable_missing_time_id(normalized_url: &str) -> i64 {
    // FNV-1a is deliberately deterministic: re-importing a record without a
    // timestamp must produce the same event hash instead of using import time.
    let mut hash = 0xcbf29ce484222325u64;
    for byte in normalized_url.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    let value = (hash & i64::MAX as u64) as i64;
    if value == 0 {
        i64::MIN + 1
    } else {
        i64::MIN | value
    }
}

fn checked_visit_time(item: &TakeoutRecordItem, normalized_url: &str) -> (i64, i64) {
    const MAX_REASONABLE_TIME_MS: i64 = 4_102_444_800_000; // 2100-01-01 UTC
    let micros = parse_json_i64(&item.time_usec)
        .or_else(|| parse_json_i64(&item.timestamp_usec))
        .or_else(|| {
            parse_json_timestamp(&item.timestamp_msec, true).and_then(|ms| ms.checked_mul(1000))
        })
        .or_else(|| {
            item.time
                .as_deref()
                .and_then(parse_iso_time)
                .and_then(|ms| ms.checked_mul(1000))
        });

    if let Some(raw_micros) = micros {
        let visit_time_ms = raw_micros / 1000;
        if raw_micros > 0 && visit_time_ms > 0 && visit_time_ms <= MAX_REASONABLE_TIME_MS {
            return (visit_time_ms, raw_micros);
        }
    }

    (0, stable_missing_time_id(normalized_url))
}

fn expanded_item_bytes(item: &TakeoutRecordItem) -> usize {
    let string_bytes = [
        item.url.as_ref(),
        item.title_url.as_ref(),
        item.virtual_url.as_ref(),
        item.title.as_ref(),
        item.header.as_ref(),
        item.time.as_ref(),
    ]
    .into_iter()
    .flatten()
    .map(String::len)
    .sum::<usize>();
    let value_bytes = [
        item.time_usec.as_ref(),
        item.timestamp_usec.as_ref(),
        item.timestamp_msec.as_ref(),
    ]
    .into_iter()
    .flatten()
    .filter_map(|value| serde_json::to_vec(value).ok())
    .map(|value| value.len())
    .sum::<usize>();
    string_bytes
        .saturating_add(value_bytes)
        .saturating_add(std::mem::size_of::<TakeoutRecordItem>())
}

fn validate_items(items: &[TakeoutRecordItem], cancelled: &TakeoutCancelProbe) -> AppResult<()> {
    let mut expanded_bytes = 0usize;
    for item in items {
        check_cancelled(cancelled, "解析")?;
        expanded_bytes = expanded_bytes
            .checked_add(expanded_item_bytes(item))
            .ok_or(AppError::MemoryPreparationLimit)?;
        if expanded_bytes > TAKEOUT_MAX_EXPANDED_BYTES {
            return Err(AppError::MemoryPreparationLimit);
        }
    }
    Ok(())
}

fn parse_takeout_items(
    file_path: &Path,
    cancelled: &TakeoutCancelProbe,
) -> AppResult<Vec<TakeoutRecordItem>> {
    check_cancelled(cancelled, "文件读取")?;
    let file_size = std::fs::metadata(file_path)?.len();
    if file_size > TAKEOUT_MAX_FILE_BYTES {
        return Err(AppError::MemoryPreparationLimit);
    }
    let file = File::open(file_path).map_err(|e| {
        AppError::Other(format!(
            "无法打开 Google Takeout 文件 {:?}: {}",
            file_path, e
        ))
    })?;
    let reader = BufReader::new(CancellationReader::new(file, cancelled.clone()));

    let document =
        serde_json::from_reader::<_, TakeoutDocument<true>>(reader).map_err(|error| {
            if cancelled() {
                AppError::VaultLocked("Google Takeout 解析已取消".into())
            } else {
                AppError::Parse(format!("Google Takeout JSON 解析失败: {error}"))
            }
        })?;
    if let Some(LimitedRecords(records)) = document.0 {
        validate_items(&records, cancelled)?;
        return Ok(records);
    }
    check_cancelled(cancelled, "解析")?;
    // Only legacy unknown wrapper names need a second, still bounded pass.
    let file = File::open(file_path)?;
    let reader = BufReader::new(CancellationReader::new(file, cancelled.clone()));
    let document =
        serde_json::from_reader::<_, TakeoutDocument<false>>(reader).map_err(|error| {
            if cancelled() {
                AppError::VaultLocked("Google Takeout 解析已取消".into())
            } else {
                AppError::Parse(format!("Google Takeout JSON 解析失败: {error}"))
            }
        })?;
    if let Some(LimitedRecords(records)) = document.0 {
        validate_items(&records, cancelled)?;
        return Ok(records);
    }

    Err(AppError::Other(format!(
        "未能从 {:?} 中解析出历史记录数据。请确认该文件为 Google Takeout 导出的 Chrome 历史记录或活动记录 JSON 归档",
        file_path.file_name().unwrap_or_default()
    )))
}

pub fn prepare_google_takeout_file(file_path: &Path) -> AppResult<PreparedTakeoutData> {
    prepare_google_takeout_file_checked(file_path, Arc::new(|| false))
}

pub fn prepare_google_takeout_file_checked(
    file_path: &Path,
    cancelled: TakeoutCancelProbe,
) -> AppResult<PreparedTakeoutData> {
    check_cancelled(&cancelled, "解析")?;
    let records = parse_takeout_items(file_path, &cancelled)?;
    validate_items(&records, &cancelled)?;
    Ok(PreparedTakeoutData {
        file_path: file_path.to_path_buf(),
        records,
        started_at: Utc::now().timestamp_millis(),
    })
}

pub fn commit_google_takeout_file(
    conn: &mut Connection,
    prepared: PreparedTakeoutData,
    cancelled: TakeoutCancelProbe,
) -> AppResult<TakeoutImportSummary> {
    let start_time = Instant::now();
    check_cancelled(&cancelled, "提交")?;
    let now = Utc::now().timestamp_millis();
    let total_parsed = prepared.records.len() as u64;
    let path = prepared.file_path.to_string_lossy().to_string();

    // Source metadata is deliberately created/updated inside the same
    // transaction as urls, visits and import_jobs.
    let tx = conn.transaction()?;
    let source_id: i64 = match tx
        .query_row(
            "SELECT id FROM sources WHERE browser = 'takeout' AND profile = 'Default'",
            [],
            |r| r.get(0),
        )
        .optional()?
    {
        Some(id) => {
            tx.execute(
                "UPDATE sources SET source_type='takeout', history_path=?1, enabled=1 WHERE id=?2",
                params![path, id],
            )?;
            id
        }
        None => {
            tx.execute(
                r#"
                INSERT INTO sources (browser, profile, source_type, history_path, enabled, last_visit_id, last_visit_time, last_sync_at)
                VALUES ('takeout', 'Default', 'takeout', ?1, 1, 0, 0, NULL)
                "#,
                params![path],
            )?;
            tx.last_insert_rowid()
        }
    };

    let before_urls: i64 = tx.query_row("SELECT COUNT(*) FROM urls", [], |row| row.get(0))?;
    let mut unknown_time_records = 0u64;
    let mut visits_inserted = 0u64;
    let mut duplicates_skipped = 0u64;
    let mut failed_count = 0i64;
    let mut max_visit_id = 0i64;
    let mut max_visit_time = 0i64;

    {
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

        for item in &prepared.records {
            check_cancelled(&cancelled, "提交")?;
            let url_str = match first_non_empty_url(item) {
                Some(url) => url,
                None => {
                    failed_count += 1;
                    continue;
                }
            };

            let lower_url = url_str.to_ascii_lowercase();
            if lower_url.starts_with("javascript:")
                || lower_url.starts_with("data:")
                || lower_url == "about:blank"
            {
                failed_count += 1;
                continue;
            }

            let title_str = item
                .title
                .as_deref()
                .or(item.header.as_deref())
                .unwrap_or_default()
                .trim();
            let norm = normalize_url(&url_str);
            let (visit_time_ms, stable_source_visit_id) =
                checked_visit_time(item, &norm.normalized_url);

            let url_id: i64 = insert_url_stmt.query_row(
                params![url_str, title_str, norm.domain, norm.normalized_url],
                |r| r.get(0),
            )?;
            if stable_source_visit_id < 0 {
                unknown_time_records += 1;
            }

            let event_hash = calculate_event_hash(
                source_id,
                stable_source_visit_id,
                visit_time_ms,
                &norm.normalized_url,
            );
            let rows_affected = insert_visit_stmt.execute(params![
                source_id,
                url_id,
                stable_source_visit_id,
                visit_time_ms,
                0,
                0,
                0,
                event_hash,
                now
            ])?;

            if rows_affected > 0 {
                visits_inserted += 1;
                max_visit_id = max_visit_id.max(stable_source_visit_id);
                max_visit_time = max_visit_time.max(visit_time_ms);
            } else {
                duplicates_skipped += 1;
            }
        }
    }

    check_cancelled(&cancelled, "提交")?;
    let after_urls: i64 = tx.query_row("SELECT COUNT(*) FROM urls", [], |row| row.get(0))?;
    let urls_inserted = (after_urls - before_urls).max(0) as u64;
    if failed_count > 0 && visits_inserted == 0 && duplicates_skipped == 0 {
        return Err(AppError::Parse(
            "Google Takeout 中没有可导入的网址，已回滚本次导入".into(),
        ));
    }
    tx.execute(
        r#"
        INSERT INTO import_jobs (
            source_id, started_at, finished_at, read_count,
            inserted_count, duplicate_count, failed_count, status
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'success')
        "#,
        params![
            source_id,
            prepared.started_at,
            Utc::now().timestamp_millis(),
            i64::try_from(total_parsed)
                .map_err(|_| AppError::Security("导入记录数量溢出".into()))?,
            i64::try_from(visits_inserted)
                .map_err(|_| AppError::Security("导入记录数量溢出".into()))?,
            i64::try_from(duplicates_skipped)
                .map_err(|_| AppError::Security("导入记录数量溢出".into()))?,
            failed_count,
        ],
    )?;
    tx.execute(
        r#"UPDATE sources
           SET last_visit_id=?1, last_visit_time=?2, last_sync_at=?3
           WHERE id=?4"#,
        params![max_visit_id, max_visit_time, now, source_id],
    )?;
    check_cancelled(&cancelled, "提交")?;
    tx.commit()?;

    let duration_ms = start_time.elapsed().as_millis() as u64;
    info!(
        "Takeout import complete: parsed {}, visits inserted {}, dupes {}, duration {}ms",
        total_parsed, visits_inserted, duplicates_skipped, duration_ms
    );

    Ok(TakeoutImportSummary {
        total_records_parsed: total_parsed,
        urls_inserted,
        visits_inserted,
        duplicates_skipped,
        invalid_records_skipped: failed_count as u64,
        unknown_time_records,
        duration_ms,
    })
}

pub fn import_google_takeout_file(
    conn: &mut Connection,
    file_path: &Path,
) -> AppResult<TakeoutImportSummary> {
    let prepared = prepare_google_takeout_file(file_path)?;
    commit_google_takeout_file(conn, prepared, Arc::new(|| false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn stream_limits_cancel_without_retrying_and_legacy_arrays_report_skips() {
        let mut limited =
            CancellationReader::new(std::io::Cursor::new(b"12345"), Arc::new(|| false));
        limited.remaining = 4;
        assert!(limited.read_to_end(&mut Vec::new()).is_err());
        let mut exact = CancellationReader::new(std::io::Cursor::new(b"1234"), Arc::new(|| false));
        exact.remaining = 4;
        assert_eq!(exact.read_to_end(&mut Vec::new()).unwrap(), 4);
        let mut cancelled =
            CancellationReader::new(std::io::Cursor::new(b"1234"), Arc::new(|| true));
        assert_ne!(
            cancelled.read_to_end(&mut Vec::new()).unwrap_err().kind(),
            io::ErrorKind::Interrupted
        );

        let root =
            std::env::temp_dir().join(format!("browsory_takeout_stream_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("legacy.json");
        std::fs::write(&path, r#"{"metadata":["not history",12],"legacyVisits":[{"url":"https://bounded-fixture.test/one","time":"2026-11-01T00:00:00Z"},{"url":"https://bounded-fixture.test/two"},{"url":"JAVASCRIPT:alert(1)"}]}"#).unwrap();
        let mut conn = Connection::open_in_memory().unwrap();
        crate::database::migrations::run_migrations(&mut conn).unwrap();
        let summary = import_google_takeout_file(&mut conn, &path).unwrap();
        assert_eq!(
            (
                summary.visits_inserted,
                summary.invalid_records_skipped,
                summary.unknown_time_records
            ),
            (2, 1, 1)
        );
        assert_eq!(summary.urls_inserted, 2);
        let repeat = import_google_takeout_file(&mut conn, &path).unwrap();
        assert_eq!((repeat.urls_inserted, repeat.duplicates_skipped), (0, 2));
        std::fs::write(&path, r#"{"Browser History":[{"url":"https://bounded-fixture.test/three","time_usec":{"unexpected":[1,2,3]}}]}"#).unwrap();
        assert!(prepare_google_takeout_file(&path).is_err());
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM visits", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            2
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn test_import_takeout_flow() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::database::migrations::run_migrations(&mut conn).unwrap();

        // Create temporary test JSON
        let temp_dir = std::env::temp_dir().join("browsory_test_takeout");
        let _ = std::fs::create_dir_all(&temp_dir);
        let sample_file = temp_dir.join("BrowserHistory.json");

        let sample_json = r#"{
            "Browser History": [
                {
                    "url": "https://rust-lang.org",
                    "title": "Rust Programming Language",
                    "time_usec": 1700000000000000
                },
                {
                    "url": "https://tauri.app",
                    "title": "Tauri Framework",
                    "time_usec": 1700000001000000
                }
            ]
        }"#;
        let mut f = std::fs::File::create(&sample_file).unwrap();
        f.write_all(sample_json.as_bytes()).unwrap();
        drop(f);

        let summary =
            import_google_takeout_file(&mut conn, &sample_file).expect("Takeout import failed");
        assert_eq!(summary.total_records_parsed, 2);
        assert_eq!(summary.urls_inserted, 2);
        assert_eq!(summary.visits_inserted, 2);

        // Verify that source was created with correct schema
        let source_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM sources WHERE browser = 'takeout'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(source_count, 1);

        // Clean up
        let _ = std::fs::remove_file(&sample_file);
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_import_takeout_chinese_and_extended_formats() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::database::migrations::run_migrations(&mut conn).unwrap();

        let temp_dir = std::env::temp_dir().join("browsory_test_takeout_zh");
        let _ = std::fs::create_dir_all(&temp_dir);
        let sample_file = temp_dir.join("历史记录.json");

        let sample_json = r#"{
            "历史记录": [
                {
                    "titleUrl": "https://bing.com",
                    "title": "必应搜索",
                    "time_usec": "1700000002000000"
                },
                {
                    "url": "https://github.com",
                    "header": "GitHub",
                    "time": "2024-01-01T12:00:00.000Z"
                }
            ]
        }"#;
        let mut f = std::fs::File::create(&sample_file).unwrap();
        f.write_all(sample_json.as_bytes()).unwrap();
        drop(f);

        let summary = import_google_takeout_file(&mut conn, &sample_file)
            .expect("Takeout Chinese format import failed");
        assert_eq!(summary.total_records_parsed, 2);
        assert_eq!(summary.visits_inserted, 2);

        // Re-importing the same records must result in 100% duplicate skips (idempotent)
        let summary2 = import_google_takeout_file(&mut conn, &sample_file)
            .expect("Takeout second import failed");
        assert_eq!(summary2.total_records_parsed, 2);
        assert_eq!(summary2.visits_inserted, 0);
        assert_eq!(summary2.duplicates_skipped, 2);

        let _ = std::fs::remove_file(&sample_file);
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn malformed_numbers_are_bounded_and_empty_url_falls_back() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::database::migrations::run_migrations(&mut conn).unwrap();
        let temp_dir = std::env::temp_dir().join(format!(
            "browsory_test_takeout_bounds_{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let sample_file = temp_dir.join("bounds.json");
        let too_long_title = "x".repeat(TAKEOUT_MAX_FIELD_BYTES + 1);
        std::fs::write(
            &sample_file,
            serde_json::json!({
                "Browser History": [{
                    "url": "",
                    "titleUrl": "https://fallback.test/",
                    "title": "fallback",
                    "time_usec": "not-a-number"
                }]
            })
            .to_string(),
        )
        .unwrap();

        let summary = import_google_takeout_file(&mut conn, &sample_file).unwrap();
        assert_eq!(summary.visits_inserted, 1);
        let (visit_time, source_visit_id): (i64, i64) = conn
            .query_row(
                "SELECT v.visit_time, v.source_visit_id FROM visits v JOIN urls u ON u.id=v.url_id WHERE u.url=?1",
                ["https://fallback.test/"],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(visit_time, 0);
        assert!(source_visit_id < 0);

        std::fs::write(
            &sample_file,
            serde_json::json!({
                "Browser History": [{"url": "https://too-long.test/", "title": too_long_title}]
            })
            .to_string(),
        )
        .unwrap();
        assert!(prepare_google_takeout_file(&sample_file).is_err());
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn missing_time_is_stably_deduplicated_and_import_metadata_is_transactional() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::database::migrations::run_migrations(&mut conn).unwrap();
        let temp_dir = std::env::temp_dir().join(format!(
            "browsory_test_takeout_missing_time_{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let sample_file = temp_dir.join("missing-time.json");
        std::fs::write(
            &sample_file,
            r#"{"Browser History":[{"url":"https://stable.test/","title":"stable"}]}"#,
        )
        .unwrap();

        let first = import_google_takeout_file(&mut conn, &sample_file).unwrap();
        let second = import_google_takeout_file(&mut conn, &sample_file).unwrap();
        assert_eq!(first.visits_inserted, 1);
        assert_eq!(second.visits_inserted, 0);
        assert_eq!(second.duplicates_skipped, 1);
        assert_eq!(
            conn.query_row("SELECT count(*) FROM import_jobs", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            2
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn cancellation_during_commit_rolls_back_source_rows_and_job() {
        use rusqlite::functions::FunctionFlags;
        use std::sync::atomic::{AtomicBool, Ordering};

        let mut conn = Connection::open_in_memory().unwrap();
        crate::database::migrations::run_migrations(&mut conn).unwrap();
        let temp_dir = std::env::temp_dir().join(format!(
            "browsory_test_takeout_cancel_{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let sample_file = temp_dir.join("cancel.json");
        std::fs::write(
            &sample_file,
            r#"{"Browser History":[
                {"url":"https://cancel.test/1","time_usec":1700000000000000},
                {"url":"https://cancel.test/2","time_usec":1700000001000000}
            ]}"#,
        )
        .unwrap();
        let prepared = prepare_google_takeout_file(&sample_file).unwrap();

        let cancelled = Arc::new(AtomicBool::new(false));
        let trigger = cancelled.clone();
        conn.create_scalar_function("cancel_takeout", 0, FunctionFlags::SQLITE_UTF8, move |_| {
            trigger.store(true, Ordering::Release);
            Ok(0i64)
        })
        .unwrap();
        conn.execute_batch(
            "CREATE TRIGGER cancel_takeout_import AFTER INSERT ON visits BEGIN SELECT cancel_takeout(); END;",
        )
        .unwrap();
        let probe = {
            let cancelled = cancelled.clone();
            Arc::new(move || cancelled.load(Ordering::Acquire))
        };
        assert!(commit_google_takeout_file(&mut conn, prepared, probe).is_err());
        assert_eq!(
            conn.query_row("SELECT count(*) FROM sources", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row("SELECT count(*) FROM urls", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row("SELECT count(*) FROM visits", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row("SELECT count(*) FROM import_jobs", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
