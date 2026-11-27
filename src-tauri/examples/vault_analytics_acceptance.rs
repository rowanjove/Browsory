//! Profile the real analytics API against an existing synthetic workload only.
//! Reports are new files; the database is placed in query_only mode.
use app_lib::{
    database::{
        repository::get_analytics_summary,
        sqlcipher::{configure_private_connection, open_encrypted_database},
    },
    security::envelope::{unwrap_master_key_with_password, VaultEnvelope},
};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::{
    error::Error,
    fs,
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
static STATEMENTS: Mutex<Vec<(String, u128)>> = Mutex::new(Vec::new());
fn profile(sql: &str, duration: Duration) {
    if let Ok(mut statements) = STATEMENTS.lock() {
        statements.push((sql.to_owned(), duration.as_millis()));
    }
}
fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(
        args.next()
            .ok_or("completed synthetic workload root required")?,
    );
    let mode = args.next().ok_or("plain or encrypted required")?;
    let label = args.next().ok_or("new report label required")?;
    if !["plain", "encrypted"].contains(&mode.as_str())
        || label.is_empty()
        || !label
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err("invalid mode or report label".into());
    }
    if fs::read(root.join(".browsory-synthetic-fixture"))? != b"synthetic-v1" {
        return Err("synthetic fixture marker required".into());
    }
    let baseline: Value = serde_json::from_slice(&fs::read(root.join("report.json"))?)?;
    if baseline["status"] != "passed" {
        return Err("completed baseline required".into());
    }
    let output = root.join(format!("analytics-{mode}-{label}.json"));
    let mut report_file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)?;
    let app = root.join(&mode);
    let mut conn = if mode == "encrypted" {
        let envelope = VaultEnvelope::load_from_dir(&app)?.ok_or("synthetic envelope missing")?;
        let key = unwrap_master_key_with_password(
            &envelope,
            "Synthetic-performance-fixture-passphrase-2026!",
        )?;
        open_encrypted_database(&app.join("archive.db"), key.as_bytes(), false)?
    } else {
        Connection::open(app.join("archive.db"))?
    };
    configure_private_connection(&conn)?;
    conn.pragma_update(None, "cache_size", -8192)?;
    let _: i64 = conn.query_row("PRAGMA mmap_size=0", [], |r| r.get(0))?;
    conn.pragma_update(None, "query_only", true)?;
    let foreign: i64 = conn.query_row(
        "SELECT count(*) FROM urls WHERE url NOT LIKE 'https://performance.test/path/%'",
        [],
        |r| r.get(0),
    )?;
    if foreign != 0 {
        return Err("non-synthetic URLs refused".into());
    }
    let visits: i64 = conn.query_row("SELECT count(*) FROM visits", [], |r| r.get(0))?;
    fn profile_event(event: rusqlite::trace::TraceEvent<'_>) {
        if let rusqlite::trace::TraceEvent::Profile(statement, duration) = event {
            profile(&statement.sql(), duration);
        }
    }
    conn.trace_v2(
        rusqlite::trace::TraceEventCodes::SQLITE_TRACE_PROFILE,
        Some(profile_event),
    )?;
    let start = Instant::now();
    let result = get_analytics_summary(&conn, None, None, None);
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.;
    conn.trace_v2(
        rusqlite::trace::TraceEventCodes::empty(),
        None::<fn(rusqlite::trace::TraceEvent<'_>)>,
    )?;
    let statements = std::mem::take(&mut *STATEMENTS.lock().map_err(|_| "profile lock poisoned")?);
    let mut timings = Vec::new();
    for (sql, milliseconds) in statements {
        let mut explain = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
        let count = explain.parameter_count();
        let now = chrono::Utc::now().timestamp_millis();
        let values = match count {
            0 => vec![],
            1 => vec![now - 30 * 86_400_000],
            2 => vec![0, now],
            _ => return Err("unexpected fixture query parameters".into()),
        };
        let plan = explain
            .query_map(rusqlite::params_from_iter(values), |r| {
                r.get::<_, String>(3)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        timings.push(json!({"sql":sql,"milliseconds":milliseconds,"plan":plan}));
    }
    let report = match result {
        Ok(analytics) => {
            assert_eq!(analytics.total_visits, visits as u64);
            assert_eq!(
                analytics
                    .hourly_distribution
                    .iter()
                    .map(|x| x.count)
                    .sum::<u64>(),
                visits as u64
            );
            assert_eq!(
                analytics.daily_trend.iter().map(|x| x.count).sum::<u64>(),
                visits as u64
            );
            assert_eq!(
                analytics
                    .weekly_heatmap
                    .iter()
                    .map(|x| x.count)
                    .sum::<u64>(),
                visits as u64
            );
            json!({"format_version":1,"status":"passed","mode":mode,"visits":visits,"elapsed_ms":elapsed_ms,"statements":timings,"analytics":analytics,"scope":"Single synthetic workload in query_only mode. No OS cache flush or cold-start claim."})
        }
        Err(error) => {
            json!({"format_version":1,"status":"failed","mode":mode,"elapsed_ms":elapsed_ms,"statements":timings,"error":error.to_string()})
        }
    };
    serde_json::to_writer_pretty(&mut report_file, &report)?;
    if report["status"] != "passed" {
        return Err("analytics workload failed; see report".into());
    }
    println!(
        "Analytics acceptance passed in {elapsed_ms:.2} ms: {}",
        output.display()
    );
    Ok(())
}
