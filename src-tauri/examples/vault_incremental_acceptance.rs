//! Add 1,000 real source visits after the completed million-row workload, then
//! exercise actual incremental preparation/commit against both archive modes.
use app_lib::{
    database::{
        repository::get_source_by_id,
        sqlcipher::{configure_private_connection, open_encrypted_database},
    },
    history::sync::{commit_source_records, prepare_source_records},
    security::envelope::{unwrap_master_key_with_password, VaultEnvelope},
};
use rusqlite::{params, Connection};
use serde_json::json;
use std::{error::Error, fs, path::PathBuf, time::Instant};
fn main() -> Result<(), Box<dyn Error>> {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or("completed synthetic workload root required")?,
    );
    if fs::read(root.join(".browsory-synthetic-fixture"))? != b"synthetic-v1" {
        return Err("synthetic marker mismatch".into());
    }
    let baseline: serde_json::Value = serde_json::from_slice(&fs::read(root.join("report.json"))?)?;
    if baseline["status"] != "passed" || baseline["rows"] != 1_000_000 {
        return Err("completed million-row baseline required".into());
    }
    let output = root.join("incremental-report.json");
    if output.exists() {
        return Err("refuse to overwrite existing incremental evidence".into());
    }
    let source = root.join("browser.db");
    let mut browser = Connection::open(&source)?;
    let count: i64 = browser.query_row("SELECT count(*) FROM visits", [], |r| r.get(0))?;
    if count != 1_000_000 {
        return Err("source is not the original million-row fixture".into());
    }
    let foreign: i64 = browser.query_row(
        "SELECT count(*) FROM urls WHERE url NOT LIKE 'https://performance.test/path/%'",
        [],
        |r| r.get(0),
    )?;
    if foreign != 0 {
        return Err("non-synthetic source URLs refused".into());
    }
    let mut archives = Vec::new();
    for mode in ["plain", "encrypted"] {
        let app = root.join(mode);
        let path = app.join("archive.db");
        let conn = if mode == "encrypted" {
            let envelope = VaultEnvelope::load_from_dir(&app)?.ok_or("fixture envelope missing")?;
            let key = unwrap_master_key_with_password(
                &envelope,
                "Synthetic-performance-fixture-passphrase-2026!",
            )?;
            open_encrypted_database(&path, key.as_bytes(), false)?
        } else {
            Connection::open(&path)?
        };
        configure_private_connection(&conn)?;
        if conn.query_row("SELECT count(*) FROM visits", [], |r| r.get::<_, i64>(0))? != 1_000_000 {
            return Err("archive is not the original baseline".into());
        }
        let foreign: i64 = conn.query_row(
            "SELECT count(*) FROM urls WHERE url NOT LIKE 'https://performance.test/path/%'",
            [],
            |r| r.get(0),
        )?;
        if foreign != 0 {
            return Err("non-synthetic destination URLs refused".into());
        }
        archives.push((mode, app, conn));
    }
    let tx = browser.transaction()?;
    {
        let mut insert = tx.prepare("INSERT INTO visits VALUES(?1,?2,?3,0,1,1000)")?;
        for id in 1_000_001..=1_001_000i64 {
            insert.execute(params![
                id,
                1 + ((id - 1) % 25_000),
                app_lib::history::unix_millis_to_webkit_micros(1_700_000_000_000) + id * 60_000_000
            ])?;
        }
    }
    tx.commit()?;
    drop(browser);
    let mut modes = Vec::new();
    for (mode, app, mut conn) in archives {
        let state = get_source_by_id(&conn, 1)?.ok_or("source missing")?;
        let start = Instant::now();
        let prepared = prepare_source_records(
            1,
            "chrome",
            &source,
            state.last_visit_time,
            state.db_fingerprint.as_deref(),
            &app.join("temp"),
        )?;
        let prepare_ms = start.elapsed().as_secs_f64() * 1000.;
        let start = Instant::now();
        let imported = commit_source_records(
            &mut conn,
            1,
            "chrome",
            "Synthetic",
            state.last_visit_time,
            prepared,
            start,
        )?;
        let commit_ms = start.elapsed().as_secs_f64() * 1000.;
        assert_eq!(imported.inserted_count, 1000);
        assert_eq!(imported.duplicate_count, 2881);
        assert_eq!(imported.failed_count, 0);
        let actual: i64 = conn.query_row("SELECT count(*) FROM visits", [], |r| r.get(0))?;
        assert_eq!(actual, 1_001_000);
        modes.push(json!({"mode":mode,"prepare_ms":prepare_ms,"commit_ms":commit_ms,"result":imported,"final_visits":actual}));
    }
    fs::write(
        &output,
        serde_json::to_vec_pretty(
            &json!({"format_version":1,"status":"passed","baseline_rows":1_000_000,"new_source_visits":1000,"modes":modes}),
        )?,
    )?;
    println!(
        "Incremental acceptance passed; evidence: {}",
        output.display()
    );
    Ok(())
}
