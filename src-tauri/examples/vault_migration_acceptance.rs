//! Upgrade a copy of the completed synthetic million-row fixture through the
//! actual vault migration entry point. Never accepts an existing destination.
use app_lib::{
    database::sqlcipher::check_encrypted_integrity,
    security::{
        backup::compute_file_sha256,
        envelope::{unwrap_master_key_with_password, VaultEnvelope},
        vault::{VaultManager, VaultStatus},
    },
};
use rusqlite::{types::ValueRef, Connection, OpenFlags};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{error::Error, fs, path::PathBuf, time::Instant};
use zeroize::Zeroizing;

const PASSWORD: &str = "Synthetic-migration-passphrase-2026!";
const NEW_PASSWORD: &str = "Synthetic-rewrapped-passphrase-2026!";
type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn digest_query(conn: &Connection, sql: &str) -> Result<Value> {
    let mut query = conn.prepare(sql)?;
    let width = query.column_count();
    let mut rows = query.query([])?;
    let mut hash = Sha256::new();
    let mut count = 0u64;
    while let Some(row) = rows.next()? {
        for col in 0..width {
            match row.get_ref(col)? {
                ValueRef::Null => hash.update([0]),
                ValueRef::Integer(n) => {
                    hash.update([1]);
                    hash.update(n.to_le_bytes());
                }
                ValueRef::Real(n) => {
                    hash.update([2]);
                    hash.update(n.to_bits().to_le_bytes());
                }
                ValueRef::Text(bytes) | ValueRef::Blob(bytes) => {
                    hash.update([if matches!(row.get_ref(col)?, ValueRef::Text(_)) {
                        3
                    } else {
                        4
                    }]);
                    hash.update((bytes.len() as u64).to_le_bytes());
                    hash.update(bytes);
                }
            }
        }
        count += 1;
    }
    Ok(json!({"rows":count,"sha256":hex::encode(hash.finalize())}))
}

fn fingerprint(conn: &Connection) -> Result<Value> {
    let mut tables = serde_json::Map::new();
    for table in ["urls", "visits", "sources", "schema_migrations"] {
        tables.insert(
            table.into(),
            digest_query(conn, &format!("SELECT * FROM {table} ORDER BY 1"))?,
        );
    }
    let schema = digest_query(
        conn,
        "SELECT type,name,tbl_name,coalesce(sql,'') FROM sqlite_schema ORDER BY type,name",
    )?;
    let fts: i64 = conn.query_row(
        "SELECT count(*) FROM urls_fts WHERE urls_fts MATCH 'searchable'",
        [],
        |r| r.get(0),
    )?;
    let foreign_keys: i64 =
        conn.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })?;
    assert_eq!(fts, 25_000);
    assert_eq!(foreign_keys, 0);
    Ok(json!({"tables":tables,"schema":schema,"fts_matches":fts,"foreign_key_errors":foreign_keys}))
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let source_root = PathBuf::from(args.next().ok_or("completed synthetic root required")?);
    let destination = PathBuf::from(args.next().ok_or("new destination required")?);
    if fs::read(source_root.join(".browsory-synthetic-fixture"))? != b"synthetic-v1" {
        return Err("synthetic marker mismatch".into());
    }
    let baseline: Value = serde_json::from_slice(&fs::read(source_root.join("report.json"))?)?;
    let increment: Value =
        serde_json::from_slice(&fs::read(source_root.join("incremental-report.json"))?)?;
    if baseline["status"] != "passed"
        || baseline["rows"] != 1_000_000
        || increment["status"] != "passed"
        || increment["new_source_visits"] != 1000
    {
        return Err("completed synthetic baseline and increment required".into());
    }
    let source = source_root.join("plain/archive.db");
    for suffix in ["-wal", "-shm", "-journal"] {
        if source
            .with_file_name(format!("archive.db{suffix}"))
            .exists()
        {
            return Err("source has side files; refuse an unproven file copy".into());
        }
    }
    let source_bytes = fs::metadata(&source)?.len();
    if source_bytes <= 256 * 1024 * 1024 {
        return Err("source is not above the large-file threshold".into());
    }
    let original_hash = compute_file_sha256(&source)?;
    let before = {
        let conn = Connection::open_with_flags(&source, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let foreign: i64 = conn.query_row(
            "SELECT count(*) FROM urls WHERE url NOT LIKE 'https://performance.test/path/%'",
            [],
            |r| r.get(0),
        )?;
        if foreign != 0 {
            return Err("non-synthetic source URLs refused".into());
        }
        let value = fingerprint(&conn)?;
        if value["tables"]["visits"]["rows"] != 1_001_000 {
            return Err("source visit count differs from completed increment".into());
        }
        value
    };
    fs::create_dir(&destination)?;
    fs::write(
        destination.join(".browsory-synthetic-fixture"),
        b"synthetic-v1",
    )?;
    let data = destination.join("data");
    fs::create_dir(&data)?;
    fs::copy(&source, data.join("archive.db"))?;
    if compute_file_sha256(&data.join("archive.db"))? != original_hash {
        return Err("synthetic copy hash mismatch".into());
    }
    let outcome = (|| -> Result<Value> {
        let vault = VaultManager::new(data.clone());
        assert_eq!(vault.status(), VaultStatus::LegacyPlaintext);
        let start = Instant::now();
        let recovery = Zeroizing::new(vault.migrate_legacy_plaintext(PASSWORD)?);
        let migrate_ms = start.elapsed().as_secs_f64() * 1000.;
        vault.confirm_recovery_key(&recovery)?;
        let after = vault.with_conn(|conn| {
            assert!(check_encrypted_integrity(conn)?);
            Ok(fingerprint(conn).expect("migrated fingerprint"))
        })?;
        assert_eq!(before, after);
        assert_eq!(
            compute_file_sha256(&data.join("archive.db.plaintext.bak"))?,
            original_hash
        );
        let env = VaultEnvelope::load_from_dir(&data)?.ok_or("envelope missing")?;
        let old_key = unwrap_master_key_with_password(&env, PASSWORD)?;
        let before_rewrap_hash = compute_file_sha256(&data.join("archive.db"))?;
        vault.change_password(PASSWORD, NEW_PASSWORD)?;
        assert_eq!(
            compute_file_sha256(&data.join("archive.db"))?,
            before_rewrap_hash,
            "passphrase change must not rewrite database pages"
        );
        let env = VaultEnvelope::load_from_dir(&data)?.ok_or("updated envelope missing")?;
        let new_key = unwrap_master_key_with_password(&env, NEW_PASSWORD)?;
        assert_eq!(old_key.as_bytes(), new_key.as_bytes());
        vault.lock()?;
        assert_eq!(vault.status(), VaultStatus::Locked);
        assert!(vault.connection().is_err());
        assert!(vault.unlock_with_password(PASSWORD).is_err());
        for _ in 0..3 {
            vault.unlock_with_password(NEW_PASSWORD)?;
            assert_eq!(
                vault.with_conn(|c| Ok(c.query_row(
                    "SELECT count(*) FROM visits",
                    [],
                    |r| r.get::<_, i64>(0)
                )?))?,
                1_001_000
            );
            vault.lock()?;
        }
        vault.unlock_with_recovery_key(&recovery)?;
        assert_eq!(
            vault.with_conn(|c| Ok(
                c.query_row("SELECT count(*) FROM visits", [], |r| r.get::<_, i64>(0))?
            ))?,
            1_001_000
        );
        vault.lock()?;
        drop(vault);
        let pairs =
            fs::read_dir(data.join("migration-recovery"))?.collect::<std::io::Result<Vec<_>>>()?;
        assert_eq!(pairs.len(), 1);
        let pair = VaultManager::new(pairs[0].path());
        assert_eq!(pair.status(), VaultStatus::Locked);
        pair.unlock_with_password(PASSWORD)?;
        let recovered = pair.with_conn(|c| Ok(fingerprint(c).expect("recovery fingerprint")))?;
        assert_eq!(recovered, before);
        pair.lock()?;
        assert_eq!(compute_file_sha256(&source)?, original_hash);
        Ok(
            json!({"migration_ms":migrate_ms,"source_bytes":source_bytes,"source_sha256":original_hash,"source_unchanged":true,"plaintext_original_retained":true,"fingerprints":after,"verified_migration_recovery_pairs":1,"password_change_preserved_database_bytes_and_master_key":true,"old_password_rejected":true,"repeated_lock_unlock_cycles":3,"recovery_key_unlock":true,"cross_device_test":false,"gui_test":false}),
        )
    })();
    let report = json!({"format_version":1,"status":if outcome.is_ok(){"passed"}else{"failed"},"workload":"large_synthetic_plaintext_migration","build":if cfg!(debug_assertions){"debug"}else{"release"},"result":outcome.as_ref().ok(),"error":outcome.as_ref().err().map(|e|e.to_string())});
    fs::write(
        destination.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    outcome?;
    println!(
        "Large migration acceptance passed: {}",
        destination.join("report.json").display()
    );
    Ok(())
}
