//! Repeatable, synthetic workload only. Refuses to reuse an existing directory.
//! cargo run --release --example vault_performance -- 1000000 <new-output-dir>
use app_lib::{
    database::{
        encrypt_migration::create_vault_database,
        migrations::run_migrations,
        models::HistoryFilter,
        repository::{get_analytics_summary, get_or_create_source, query_history},
        sqlcipher::{
            check_encrypted_integrity, configure_private_connection, open_encrypted_database,
        },
    },
    history::{
        parser::unix_millis_to_webkit_micros,
        sync::{commit_source_records, prepare_source_records},
    },
    security::{
        backup::{execute_encrypted_backup, restore_encrypted_backup_flow},
        envelope::{unwrap_master_key_with_password, MasterKey, VaultEnvelope},
        vault::VaultManager,
    },
};
use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};
use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    time::Instant,
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
const PASSWORD: &str = "Synthetic-performance-fixture-passphrase-2026!";

fn timed<T>(metrics: &mut Vec<Value>, name: &str, work: impl FnOnce() -> Result<T>) -> Result<T> {
    eprintln!("starting {name}");
    let start = Instant::now();
    match work() {
        Ok(value) => {
            let elapsed = start.elapsed().as_secs_f64() * 1000.;
            metrics.push(json!({"stage":name,"milliseconds":elapsed}));
            eprintln!("completed {name}: {elapsed:.2} ms");
            Ok(value)
        }
        Err(error) => {
            metrics.push(json!({"stage":name,"error":error.to_string(),"milliseconds":start.elapsed().as_secs_f64()*1000.}));
            Err(error)
        }
    }
}
fn configure(conn: &Connection) -> Result<()> {
    configure_private_connection(conn)?;
    conn.pragma_update(None, "cache_size", -8192)?;
    let _: u64 = conn.query_row("PRAGMA mmap_size=0", [], |r| r.get(0))?;
    Ok(())
}
fn fixture(path: &Path, rows: u64) -> Result<()> {
    let mut conn = Connection::open(path)?;
    configure(&conn)?;
    conn.execute_batch("CREATE TABLE urls(id INTEGER PRIMARY KEY,url TEXT,title TEXT);CREATE TABLE visits(id INTEGER PRIMARY KEY,url INTEGER,visit_time INTEGER,from_visit INTEGER,transition INTEGER,visit_duration INTEGER);CREATE INDEX visits_time ON visits(visit_time);")?;
    let tx = conn.transaction()?;
    tx.execute("WITH RECURSIVE n(i) AS(SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i<?1) INSERT INTO urls SELECT i,'https://performance.test/path/'||i,'Vault searchable benchmark topic '||i FROM n",[rows.min(25_000)])?;
    tx.execute("WITH RECURSIVE n(i) AS(SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i<?1) INSERT INTO visits SELECT i,1+((i-1)%?2),?3+i*60000000,0,1,1000 FROM n",rusqlite::params![rows,rows.min(25_000),unix_millis_to_webkit_micros(1_700_000_000_000)])?;
    tx.commit()?;
    Ok(())
}
fn exercise(
    root: &Path,
    source: &Path,
    rows: u64,
    encrypted: bool,
    metrics: &mut Vec<Value>,
) -> Result<Value> {
    let name = if encrypted { "encrypted" } else { "plain" };
    let app = root.join(name);
    fs::create_dir(&app)?;
    let (key, envelope): (Option<MasterKey>, Option<VaultEnvelope>) = if encrypted {
        let (env, _, key) = timed(metrics, "encrypted.create_and_kdf", || {
            Ok(create_vault_database(&app, PASSWORD)?)
        })?;
        (Some(key), Some(env))
    } else {
        (None, None)
    };
    let path = app.join("archive.db");
    let mut conn = if let Some(key) = &key {
        open_encrypted_database(&path, key.as_bytes(), false)?
    } else {
        let mut conn = Connection::open(&path)?;
        configure(&conn)?;
        run_migrations(&mut conn)?;
        conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get::<_, String>(0))?;
        conn
    };
    configure(&conn)?;
    let registered = get_or_create_source(
        &conn,
        "chrome",
        "Synthetic",
        "sqlite",
        &source.to_string_lossy(),
    )?;
    let prepared = timed(metrics, &format!("{name}.initial_prepare"), || {
        Ok(prepare_source_records(
            registered.id,
            "chrome",
            source,
            0,
            None,
            &app.join("temp"),
        )?)
    })?;
    let uses_record_spool = prepared.records.is_empty();
    let imported = timed(metrics, &format!("{name}.initial_commit"), || {
        Ok(commit_source_records(
            &mut conn,
            registered.id,
            "chrome",
            "Synthetic",
            0,
            prepared,
            Instant::now(),
        )?)
    })?;
    assert_eq!(u64::from(imported.inserted_count), rows);
    assert_eq!(imported.failed_count, 0);
    let source_state =
        app_lib::database::repository::get_source_by_id(&conn, registered.id)?.unwrap();
    let incremental = timed(metrics, &format!("{name}.overlap_prepare"), || {
        Ok(prepare_source_records(
            registered.id,
            "chrome",
            source,
            source_state.last_visit_time,
            source_state.db_fingerprint.as_deref(),
            &app.join("temp"),
        )?)
    })?;
    let overlap = timed(metrics, &format!("{name}.overlap_commit"), || {
        Ok(commit_source_records(
            &mut conn,
            registered.id,
            "chrome",
            "Synthetic",
            source_state.last_visit_time,
            incremental,
            Instant::now(),
        )?)
    })?;
    assert_eq!(overlap.inserted_count, 0);
    assert!(overlap.duplicate_count > 0);
    for iteration in 0..5 {
        let page = timed(metrics, &format!("{name}.history_page.{iteration}"), || {
            Ok(query_history(
                &conn,
                &HistoryFilter {
                    limit: Some(100),
                    ..Default::default()
                },
            )?)
        })?;
        assert_eq!(page.items.len(), 100.min(rows as usize));
        let search = timed(
            metrics,
            &format!("{name}.history_search.{iteration}"),
            || {
                Ok(query_history(
                    &conn,
                    &HistoryFilter {
                        search: Some("searchable".into()),
                        limit: Some(100),
                        ..Default::default()
                    },
                )?)
            },
        )?;
        assert!(!search.items.is_empty());
        let fts: i64 = timed(metrics, &format!("{name}.fts.{iteration}"), || {
            Ok(conn.query_row(
                "SELECT count(*) FROM urls_fts WHERE urls_fts MATCH 'searchable'",
                [],
                |r| r.get(0),
            )?)
        })?;
        assert_eq!(fts, rows.min(25_000) as i64);
    }
    let analytics = timed(metrics, &format!("{name}.analytics"), || {
        Ok(get_analytics_summary(&conn, None, None, None)?)
    })?;
    let snapshot = app.join("snapshot.db");
    timed(metrics, &format!("{name}.snapshot_vacuum"), || {
        conn.execute("VACUUM main INTO ?1", [snapshot.to_string_lossy().as_ref()])?;
        Ok(())
    })?;
    let snapshot_conn = timed(metrics, &format!("{name}.snapshot_reopen"), || {
        Ok(if let Some(key) = &key {
            open_encrypted_database(&snapshot, key.as_bytes(), false)?
        } else {
            Connection::open(&snapshot)?
        })
    })?;
    let snapshot_rows: i64 =
        snapshot_conn.query_row("SELECT count(*) FROM visits", [], |r| r.get(0))?;
    assert_eq!(snapshot_rows, rows as i64);
    drop(snapshot_conn);
    let mut backup_result = Value::Null;
    if let (Some(key), Some(envelope)) = (&key, &envelope) {
        let package = timed(metrics, "encrypted.package_backup", || {
            execute_encrypted_backup(
                &conn,
                &app,
                &app.join("backups"),
                "performance",
                None,
                key,
                envelope,
            )
            .map_err(Into::into)
        })?;
        let target = root.join("restored");
        fs::create_dir(&target)?;
        let target_vault = VaultManager::new(target.clone());
        let _ = target_vault.create_vault(PASSWORD)?;
        let old_key = target_vault.master_key().unwrap();
        let old_env = target_vault.envelope().unwrap();
        target_vault.lock()?;
        timed(metrics, "encrypted.package_restore", || {
            restore_encrypted_backup_flow(
                Path::new(&package.file_path),
                &target,
                Some(PASSWORD),
                None,
                Some(&old_key),
                Some(&old_env),
                || Ok(()),
            )?;
            Ok(())
        })?;
        drop(target_vault);
        let restored = VaultManager::new(target);
        restored.unlock_with_password(PASSWORD)?;
        let count: i64 = restored
            .with_conn(|c| Ok(c.query_row("SELECT count(*) FROM visits", [], |r| r.get(0))?))?;
        assert_eq!(count, rows as i64);
        restored.lock()?;
        backup_result = json!({"bytes":package.file_size_bytes,"restored_visits":count});
    }
    let sqlite_version: String = conn.query_row("SELECT sqlite_version()", [], |r| r.get(0))?;
    let sqlcipher_version: Option<String> = conn
        .query_row("PRAGMA cipher_version", [], |r| r.get(0))
        .optional()?;
    let busy: i64 = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))?;
    assert_eq!(busy, 0);
    drop(conn);
    for iteration in 0..3 {
        if let Some(envelope) = &envelope {
            let unwrapped = timed(
                metrics,
                &format!("encrypted.unlock_kdf.{iteration}"),
                || unwrap_master_key_with_password(envelope, PASSWORD).map_err(Into::into),
            )?;
            let opened = timed(
                metrics,
                &format!("encrypted.connection_reopen.{iteration}"),
                || Ok(open_encrypted_database(&path, unwrapped.as_bytes(), false)?),
            )?;
            drop(opened);
        } else {
            let opened = timed(
                metrics,
                &format!("plain.connection_reopen.{iteration}"),
                || {
                    let c = Connection::open(&path)?;
                    c.query_row("SELECT count(*) FROM sqlite_schema", [], |r| {
                        r.get::<_, i64>(0)
                    })?;
                    Ok(c)
                },
            )?;
            drop(opened);
        }
    }
    if let Some(key) = &key {
        let c = open_encrypted_database(&path, key.as_bytes(), false)?;
        assert!(check_encrypted_integrity(&c)?);
    }
    Ok(
        json!({"mode":name,"sqlite_version":sqlite_version,"sqlcipher_version":sqlcipher_version,"initial_import":imported,"overlap_import":overlap,"uses_record_spool":uses_record_spool,"database_bytes":fs::metadata(path)?.len(),"snapshot_visits":snapshot_rows,"analytics":analytics,"package":backup_result}),
    )
}
fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let rows: u64 = args.next().unwrap_or_else(|| "1000000".into()).parse()?;
    if !(100..=2_000_000).contains(&rows) {
        return Err("rows must be between 100 and 2000000".into());
    }
    let root = args.next().map(PathBuf::from).unwrap_or_else(|| {
        std::env::temp_dir().join(format!("browsory_performance_{}", uuid::Uuid::new_v4()))
    });
    fs::create_dir(&root)?;
    fs::write(root.join(".browsory-synthetic-fixture"), b"synthetic-v1")?;
    let mut metrics = Vec::new();
    let source = root.join("browser.db");
    let outcome = (|| -> Result<Vec<Value>> {
        timed(&mut metrics, "fixture.generate", || fixture(&source, rows))?;
        let plain = exercise(&root, &source, rows, false, &mut metrics)?;
        let encrypted = exercise(&root, &source, rows, true, &mut metrics)?;
        Ok(vec![plain, encrypted])
    })();
    let mut hardware = sysinfo::System::new();
    hardware.refresh_memory();
    hardware.refresh_cpu_all();
    let report = json!({"format_version":1,"rows":rows,"status":if outcome.is_ok(){"passed"}else{"failed"},"build":if cfg!(debug_assertions){"debug"}else{"release"},"os":std::env::consts::OS,"architecture":std::env::consts::ARCH,"cpu":hardware.cpus().first().map(|c|c.brand()),"physical_memory_bytes":hardware.total_memory(),"metrics":metrics,"workloads":outcome.as_ref().ok(),"error":outcome.as_ref().err().map(|e|e.to_string()),"limitations":["Connection reopen does not flush OS page cache; it is not a power-on cold-start measurement.","Initial/overlap imports use the actual source preparation and commit code. Data are entirely synthetic.","Snapshot VACUUM uses the same SQL for both modes; ZIP package backup/restore is measured separately only for the encrypted app.","Single machine, one workload per mode; timings are not universal overhead percentages."]});
    fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("report: {}", root.join("report.json").display());
    outcome.map(|_| ())
}
