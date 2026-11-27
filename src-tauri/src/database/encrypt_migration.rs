use chrono::Utc;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use tracing::{info, warn};
use zeroize::Zeroizing;

use super::migrations::run_migrations;
use super::space::{migration_requirement, require_available_space};
use super::sqlcipher::{
    check_encrypted_integrity, open_encrypted_database, verify_sqlcipher_support,
};
use crate::error::{AppError, AppResult};
use crate::security::atomic_file::write_atomic;
use crate::security::backup::compute_file_sha256;
use crate::security::envelope::{
    create_auto_vault_envelope, create_vault_envelope, MasterKey, VaultEnvelope,
};

pub const SQLITE_HEADER_MAGIC: &[u8; 16] = b"SQLite format 3\0";
const PENDING_ENVELOPE: &str = "vault.envelope.pending.json";

pub fn is_plain_sqlite_database(db_path: &Path) -> bool {
    let mut header = [0u8; 16];
    File::open(db_path)
        .and_then(|mut f| f.read_exact(&mut header))
        .is_ok()
        && &header == SQLITE_HEADER_MAGIC
}

pub fn verify_sqlite_integrity(db_path: &Path) -> AppResult<bool> {
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    super::sqlcipher::configure_private_connection(&conn)?;
    let mut stmt = conn.prepare("PRAGMA integrity_check;")?;
    let results = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(results == ["ok"])
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationStateMarker {
    pub stage: String,
    // Retained to read older markers. Never used as a filesystem authority.
    pub pre_backup_path: String,
    pub created_at: i64,
    #[serde(default)]
    pub db_sha256: Option<String>,
    #[serde(default)]
    pub envelope_sha256: Option<String>,
}

impl MigrationStateMarker {
    pub fn marker_path(data_dir: &Path) -> PathBuf {
        data_dir.join("migration.state.json")
    }

    pub fn save(&self, data_dir: &Path) -> AppResult<()> {
        write_atomic(&Self::marker_path(data_dir), &serde_json::to_vec(self)?)?;
        Ok(())
    }

    pub fn load(data_dir: &Path) -> AppResult<Option<Self>> {
        let path = Self::marker_path(data_dir);
        if !path.try_exists()? {
            return Ok(None);
        }
        let file = File::open(&path)?;
        let mut bytes = Vec::new();
        file.take(16 * 1024 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > 16 * 1024 {
            return Err(AppError::Security(
                "迁移状态文件超出大小限制，保留现场".into(),
            ));
        }
        Ok(Some(serde_json::from_slice(&bytes)?))
    }

    fn delete(data_dir: &Path) -> AppResult<()> {
        fs::remove_file(Self::marker_path(data_dir))?;
        Ok(())
    }
}

fn migration_error(message: &str) -> AppError {
    AppError::Migration(message.into())
}

fn optional_file_size(path: &Path) -> AppResult<u64> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.len()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error.into()),
    }
}

fn source_wal_path(source: &Path) -> AppResult<PathBuf> {
    let file_name = source
        .file_name()
        .ok_or_else(|| migration_error("原始数据库路径缺少文件名"))?;
    let mut wal_name = file_name.to_os_string();
    wal_name.push("-wal");
    Ok(source.with_file_name(wal_name))
}

fn migration_space_preflight(data_dir: &Path, source: &Path) -> AppResult<()> {
    let main_bytes = fs::metadata(source)?.len();
    let wal_bytes = optional_file_size(&source_wal_path(source)?)?;
    let required = migration_requirement(main_bytes, wal_bytes)?;
    require_available_space(data_dir, required)
}

#[cfg(test)]
fn migration_space_preflight_with<F>(
    data_dir: &Path,
    source: &Path,
    available_space: F,
) -> AppResult<()>
where
    F: FnOnce(&Path) -> std::io::Result<u64>,
{
    let main_bytes = fs::metadata(source)?.len();
    let wal_bytes = optional_file_size(&source_wal_path(source)?)?;
    let required = migration_requirement(main_bytes, wal_bytes)?;
    let available = available_space(data_dir)?;
    if available < required {
        return Err(migration_error("数据库迁移前可用空间不足"));
    }
    Ok(())
}

fn checkpoint_truncate(conn: &Connection) -> AppResult<()> {
    let (busy, _, _): (i64, i64, i64) =
        conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
    if busy != 0 {
        return Err(migration_error("数据库正忙，WAL 无法完整合并，已中止操作"));
    }
    Ok(())
}

fn matches_hash(path: &Path, expected: &Option<String>) -> AppResult<bool> {
    match expected {
        Some(hash) if path.is_file() => Ok(compute_file_sha256(path)? == *hash),
        _ => Ok(false),
    }
}

fn complete_marker(data_dir: &Path, marker: &mut MigrationStateMarker) -> AppResult<()> {
    // Once this state is durable, the vault may be used and its DB hash may
    // legitimately change. Failure to unlink a committed journal is harmless.
    marker.stage = "committed".into();
    marker.save(data_dir)?;
    if let Err(error) = MigrationStateMarker::delete(data_dir) {
        warn!("已提交的保险库状态记录暂未清理: {}", error);
    }
    Ok(())
}

fn retain_staging(data_dir: &Path) -> AppResult<()> {
    let names = [
        "archive.staging.db",
        "archive.staging.db-wal",
        "archive.staging.db-shm",
        PENDING_ENVELOPE,
    ];
    if names.iter().any(|name| data_dir.join(name).exists()) {
        let retained = data_dir
            .join("migration-interrupted")
            .join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&retained)?;
        for name in names {
            let path = data_dir.join(name);
            if path.exists() {
                fs::rename(&path, retained.join(name))?;
            }
        }
    }
    Ok(())
}

/// Recover only a recorded transaction. A retained plaintext original after a
/// successful migration is not evidence of a crash. Ambiguous files are preserved.
pub fn recover_from_interrupted_migration(data_dir: &Path) -> AppResult<bool> {
    let source = data_dir.join("archive.db");
    let staging = data_dir.join("archive.staging.db");
    let original = data_dir.join("archive.db.plaintext.bak");
    let pending = data_dir.join(PENDING_ENVELOPE);
    let envelope_path = VaultEnvelope::envelope_path(data_dir);
    let mut marker = match MigrationStateMarker::load(data_dir)? {
        Some(marker) => marker,
        None => {
            if staging.exists() || pending.exists() {
                return Err(migration_error(
                    "发现无状态记录的迁移文件；已保留全部文件，请先恢复",
                ));
            }
            return Ok(false);
        }
    };
    if !matches!(
        marker.stage.as_str(),
        "creating"
            | "creation_verified"
            | "started"
            | "staged_verified"
            | "source_retired"
            | "database_installed"
            | "committed"
    ) {
        return Err(migration_error("未知的迁移阶段；已保留全部文件"));
    }
    if marker.stage == "committed" {
        if !source.is_file()
            || is_plain_sqlite_database(&source)
            || VaultEnvelope::load_from_path(&envelope_path)
                .map_err(AppError::Security)?
                .is_none()
        {
            return Err(migration_error(
                "已提交的保险库文件缺失或状态冲突，保留现场",
            ));
        }
        if let Err(error) = MigrationStateMarker::delete(data_dir) {
            warn!("保留已提交的保险库状态记录: {}", error);
        }
        return Ok(true);
    }
    if marker.stage == "creating" && !source.exists() && !envelope_path.exists() {
        retain_staging(data_dir)?;
        MigrationStateMarker::delete(data_dir)?;
        return Ok(true);
    }
    if marker.stage == "creation_verified" && !source.exists() && !envelope_path.exists() {
        if !matches_hash(&staging, &marker.db_sha256)?
            || !matches_hash(&pending, &marker.envelope_sha256)?
        {
            return Err(migration_error("新建保险库暂存文件无法匹配，保留现场"));
        }
        VaultEnvelope::load_from_path(&pending).map_err(AppError::Security)?;
        fs::rename(&staging, &source)?;
        // Keep creation_verified until the next step; recovery also recognizes
        // this state when archive.db has already been installed.
    }

    // Crash after installing the encrypted DB: complete the matching pair only
    // when it is exactly the pair verified before the switch.
    if source.exists() && !is_plain_sqlite_database(&source) {
        if !matches_hash(&source, &marker.db_sha256)? {
            return Err(migration_error(
                "迁移后的数据库无法与状态记录匹配；已保留恢复现场",
            ));
        }
        if envelope_path.exists() {
            if !matches_hash(&envelope_path, &marker.envelope_sha256)? {
                return Err(migration_error("数据库与信封不匹配；已保留恢复现场"));
            }
            VaultEnvelope::load_from_path(&envelope_path).map_err(AppError::Security)?;
        } else {
            if !matches_hash(&pending, &marker.envelope_sha256)? {
                return Err(migration_error("缺少匹配的迁移信封；已保留恢复现场"));
            }
            VaultEnvelope::load_from_path(&pending).map_err(AppError::Security)?;
            fs::rename(&pending, &envelope_path)?;
        }
        complete_marker(data_dir, &mut marker)?;
        return Ok(true);
    }

    if envelope_path.exists() {
        return Err(migration_error(
            "明文数据库与保险库信封状态冲突；禁止自动覆盖",
        ));
    }
    if !source.exists() {
        if !is_plain_sqlite_database(&original) {
            return Err(migration_error(
                "迁移中断且原始数据库缺失；禁止创建空库或清理现场",
            ));
        }
        // Older builds might have retained an uncheckpointed WAL. Restore it
        // together with its original database, never attach it to encrypted data.
        let wal_backup = data_dir.join("archive.db-wal.plaintext.bak");
        if data_dir.join("archive.db-wal").exists() {
            return Err(migration_error("存在归属不明的 WAL，保留现场等待恢复"));
        }
        fs::rename(&original, &source)?;
        if wal_backup.exists() {
            fs::rename(wal_backup, data_dir.join("archive.db-wal"))?;
        }
    }
    if !is_plain_sqlite_database(&source) || !verify_sqlite_integrity(&source)? {
        return Err(migration_error("原始数据库未通过校验，保留迁移文件"));
    }

    // Preserve staging evidence rather than deleting the only possible copy.
    retain_staging(data_dir)?;
    MigrationStateMarker::delete(data_dir)?;
    Ok(true)
}

fn install_verified_pair(
    data_dir: &Path,
    marker: &mut MigrationStateMarker,
    retire_plaintext: bool,
) -> AppResult<()> {
    install_verified_pair_observed(data_dir, marker, retire_plaintext, |_| Ok(()))
}

fn install_verified_pair_observed<F>(
    data_dir: &Path,
    marker: &mut MigrationStateMarker,
    retire_plaintext: bool,
    mut observe: F,
) -> AppResult<()>
where
    F: FnMut(&str) -> AppResult<()>,
{
    let result = (|| {
        if retire_plaintext {
            fs::rename(
                data_dir.join("archive.db"),
                data_dir.join("archive.db.plaintext.bak"),
            )?;
            marker.stage = "source_retired".into();
            marker.save(data_dir)?;
        }
        fs::rename(
            data_dir.join("archive.staging.db"),
            data_dir.join("archive.db"),
        )?;
        marker.stage = "database_installed".into();
        marker.save(data_dir)?;
        observe("database_installed")?;
        fs::rename(
            data_dir.join(PENDING_ENVELOPE),
            VaultEnvelope::envelope_path(data_dir),
        )?;
        complete_marker(data_dir, marker)
    })();
    if let Err(error) = result {
        // If the DB has been installed, finish the matching pair now and still
        // return the generated recovery key to this caller. Otherwise rollback
        // to plaintext and let the user safely retry with a fresh key.
        recover_from_interrupted_migration(data_dir)?;
        if !data_dir.join("archive.db").exists()
            || is_plain_sqlite_database(&data_dir.join("archive.db"))
            || !VaultEnvelope::envelope_path(data_dir).exists()
        {
            return Err(error);
        }
    }
    Ok(())
}

/// A new vault is built and checked in staging before either live file appears.
pub fn create_vault_database(
    data_dir: &Path,
    password: &str,
) -> AppResult<(VaultEnvelope, String, MasterKey)> {
    let now = Utc::now().timestamp_millis();
    let (envelope, recovery, key) =
        create_vault_envelope(password, None, now).map_err(AppError::Security)?;
    create_vault_database_with_envelope(data_dir, envelope, key, now)
        .map(|(envelope, key)| (envelope, recovery, key))
}

/// Creates a passive, DPAPI-protected encrypted vault without user prompts.
pub fn create_passive_vault_database(data_dir: &Path) -> AppResult<(VaultEnvelope, MasterKey)> {
    let now = Utc::now().timestamp_millis();
    let (envelope, key) = create_auto_vault_envelope(now).map_err(AppError::Security)?;
    create_vault_database_with_envelope(data_dir, envelope, key, now)
}

fn create_vault_database_with_envelope(
    data_dir: &Path,
    envelope: VaultEnvelope,
    key: MasterKey,
    now: i64,
) -> AppResult<(VaultEnvelope, MasterKey)> {
    for name in [
        "archive.db",
        "archive.staging.db",
        "archive.db.plaintext.bak",
        PENDING_ENVELOPE,
        "vault.envelope.json",
        "migration.state.json",
    ] {
        if data_dir.join(name).exists() {
            return Err(migration_error("已有数据库或恢复文件，禁止覆盖创建保险库"));
        }
    }
    let mut marker = MigrationStateMarker {
        stage: "creating".into(),
        pre_backup_path: String::new(),
        created_at: now,
        db_sha256: None,
        envelope_sha256: None,
    };
    marker.save(data_dir)?;
    let staging = data_dir.join("archive.staging.db");
    {
        let mut conn = open_encrypted_database(&staging, key.as_bytes(), true)?;
        run_migrations(&mut conn)?;
        if !check_encrypted_integrity(&conn)? {
            return Err(migration_error("新建保险库校验失败"));
        }
        checkpoint_truncate(&conn)?;
    }
    fs::OpenOptions::new()
        .write(true)
        .open(&staging)?
        .sync_all()?;
    let pending = data_dir.join(PENDING_ENVELOPE);
    write_atomic(&pending, &serde_json::to_vec_pretty(&envelope)?)?;
    marker.db_sha256 = Some(compute_file_sha256(&staging)?);
    marker.envelope_sha256 = Some(compute_file_sha256(&pending)?);
    marker.stage = "creation_verified".into();
    marker.save(data_dir)?;
    install_verified_pair(data_dir, &mut marker, false)?;
    Ok((envelope, key))
}

fn quote_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Exact row comparison using a non-null unique primary key avoids sorting
/// copies of every BLOB in RAM. Nullable legacy primary keys retain the full
/// multiset comparison below, since NULL keys need not identify one row.
fn verify_primary_key_table(conn: &Connection, name: &str, columns: &[String]) -> AppResult<bool> {
    let mut statement = conn.prepare(
        "SELECT name,type,\"notnull\" FROM pragma_table_info(?1,'main') WHERE pk>0 ORDER BY pk",
    )?;
    let keys = statement
        .query_map([name], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, bool>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if keys.is_empty() {
        return Ok(false);
    }
    let has_pk_index: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_index_list(?1,'main') WHERE origin='pk')",
        [name],
        |r| r.get(0),
    )?;
    let rowid_alias = keys.len() == 1 && keys[0].1.eq_ignore_ascii_case("INTEGER") && !has_pk_index;
    if !rowid_alias && keys.iter().any(|(_, _, not_null)| !not_null) {
        return Ok(false);
    }
    let table = quote_identifier(name);
    let key_match = keys
        .iter()
        .map(|(key, _, _)| {
            let key = quote_identifier(key);
            format!("e.{key}=m.{key}")
        })
        .collect::<Vec<_>>()
        .join(" AND ");
    let values_match = columns
        .iter()
        .map(|column| {
            let c = quote_identifier(column);
            format!(
                "(e.{c} COLLATE BINARY IS m.{c} COLLATE BINARY AND typeof(e.{c})=typeof(m.{c}))"
            )
        })
        .collect::<Vec<_>>()
        .join(" AND ");
    let different:bool=conn.query_row(&format!("SELECT (SELECT count(*) FROM main.{table})!=(SELECT count(*) FROM encrypted.{table}) OR EXISTS(SELECT 1 FROM main.{table} m WHERE NOT EXISTS(SELECT 1 FROM encrypted.{table} e WHERE {key_match} AND {values_match}))"),[],|r|r.get(0))?;
    if different {
        return Err(migration_error(
            "导出前后主键行内容或数量不一致，保留原始数据",
        ));
    }
    Ok(true)
}

/// Validate the full schema and every logical table, including future tables.
/// FTS shadow storage may be rebuilt by SQLCipher and is not compared bytewise.
pub(crate) fn verify_export(conn: &Connection) -> AppResult<()> {
    type SchemaRow = (String, String, String, Option<String>);
    let schema = |name: &str| -> AppResult<Vec<SchemaRow>> {
        let mut stmt = conn.prepare(&format!(
            "SELECT type, name, tbl_name, sql FROM {name}.sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name"
        ))?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    };
    if schema("main")? != schema("encrypted")? {
        return Err(migration_error(
            "导出前后 schema（表、索引、触发器、视图）不一致",
        ));
    }
    let mut stmt = conn.prepare(
        "SELECT name FROM pragma_table_list WHERE schema='main' AND type IN ('table','virtual') AND name NOT LIKE 'sqlite_%' ORDER BY name"
    )?;
    let mut tables = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let sequence: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE name='sqlite_sequence'",
        [],
        |r| r.get(0),
    )?;
    if sequence != 0 {
        tables.push("sqlite_sequence".into());
    }
    for table in tables {
        let quoted = quote_identifier(&table);
        let column_names = conn
            .prepare(&format!("SELECT * FROM main.{quoted} LIMIT 0"))?
            .column_names()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if verify_primary_key_table(conn, &table, &column_names)? {
            continue;
        }
        let table = quoted;
        let columns = column_names.len();
        let grouping = (1..=columns)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let different: bool = conn.query_row(
            &format!(
                "SELECT EXISTS(SELECT *,count(*) FROM main.{table} GROUP BY {grouping}
                           EXCEPT SELECT *,count(*) FROM encrypted.{table} GROUP BY {grouping})
             OR EXISTS(SELECT *,count(*) FROM encrypted.{table} GROUP BY {grouping}
                       EXCEPT SELECT *,count(*) FROM main.{table} GROUP BY {grouping})"
            ),
            [],
            |r| r.get(0),
        )?;
        if different {
            return Err(migration_error(
                "导出前后数据内容或数量不一致，保留原始数据",
            ));
        }
    }
    Ok(())
}

/// Export into an independent encrypted file, validate, retain an encrypted
/// recovery copy, and switch with a restart-recoverable journal.
pub fn migrate_plaintext_to_sqlcipher(
    data_dir: &Path,
    password: &str,
) -> AppResult<(VaultEnvelope, String)> {
    migrate_with_space_preflight(data_dir, password, migration_space_preflight)
}

/// Silently migrates a legacy plaintext database into a DPAPI-managed SQLCipher
/// vault. The plaintext backup remains until the user confirms a recovery key and
/// explicitly retires the backup through the verified retirement flow.
pub fn migrate_plaintext_to_sqlcipher_passive(
    data_dir: &Path,
) -> AppResult<(VaultEnvelope, MasterKey)> {
    let now = Utc::now().timestamp_millis();
    let (envelope, master_key) = create_auto_vault_envelope(now).map_err(AppError::Security)?;
    let committed = migrate_with_envelope(
        data_dir,
        envelope,
        master_key,
        now,
        migration_space_preflight,
    )?;
    Ok(committed)
}

fn migrate_with_space_preflight(
    data_dir: &Path,
    password: &str,
    space_preflight: impl FnOnce(&Path, &Path) -> AppResult<()>,
) -> AppResult<(VaultEnvelope, String)> {
    let now = Utc::now().timestamp_millis();
    let (envelope, recovery_key, master_key) =
        create_vault_envelope(password, None, now).map_err(AppError::Security)?;
    let (envelope, _) =
        migrate_with_envelope(data_dir, envelope, master_key, now, space_preflight)?;
    Ok((envelope, recovery_key))
}

fn migrate_with_envelope(
    data_dir: &Path,
    envelope: VaultEnvelope,
    master_key: MasterKey,
    now: i64,
    space_preflight: impl FnOnce(&Path, &Path) -> AppResult<()>,
) -> AppResult<(VaultEnvelope, MasterKey)> {
    let source = data_dir.join("archive.db");
    let staging = data_dir.join("archive.staging.db");
    let original = data_dir.join("archive.db.plaintext.bak");
    let pending = data_dir.join(PENDING_ENVELOPE);
    if MigrationStateMarker::load(data_dir)?.is_some()
        || staging.exists()
        || original.exists()
        || pending.exists()
        || VaultEnvelope::envelope_path(data_dir).exists()
    {
        return Err(migration_error(
            "存在保险库或未处理的迁移文件，禁止覆盖或重复迁移",
        ));
    }
    if !is_plain_sqlite_database(&source) || !verify_sqlite_integrity(&source)? {
        return Err(migration_error(
            "原始数据库不存在、并非明文 SQLite 或完整性检查失败",
        ));
    }
    // Do this before KDF work, marker persistence, WAL checkpointing, or any
    // staging allocation.  A failed preflight must leave the source and its
    // WAL byte-for-byte untouched and make retry safe.
    space_preflight(data_dir, &source)?;
    let mut marker = MigrationStateMarker {
        stage: "started".into(),
        pre_backup_path: String::new(),
        created_at: now,
        db_sha256: None,
        envelope_sha256: None,
    };
    marker.save(data_dir)?;
    let mut source_conn = Connection::open_with_flags(&source, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    super::sqlcipher::configure_private_connection(&source_conn)?;
    verify_sqlcipher_support(&source_conn)?;
    source_conn.busy_timeout(std::time::Duration::from_secs(5))?;
    let (busy, _, _): (i64, i64, i64) =
        source_conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
    if busy != 0 {
        return Err(migration_error(
            "原始数据库正忙，WAL 无法完整合并，已中止迁移",
        ));
    }
    let user_version: i64 = source_conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let application_id: i64 =
        source_conn.pragma_query_value(None, "application_id", |r| r.get(0))?;
    let auto_vacuum: i64 = source_conn.pragma_query_value(None, "auto_vacuum", |r| r.get(0))?;
    let key_hex = Zeroizing::new(hex::encode(master_key.as_bytes()));
    let raw_key = Zeroizing::new(format!("x'{}'", key_hex.as_str()));
    // ATTACH inherits the source connection's no-CREATE flag. Reserve a new,
    // empty staging file explicitly without permitting source recreation.
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staging)?;
    source_conn.execute(
        "ATTACH DATABASE ?1 AS encrypted KEY ?2",
        rusqlite::params![staging.to_string_lossy().as_ref(), raw_key.as_str()],
    )?;
    source_conn.execute_batch(&format!("PRAGMA encrypted.auto_vacuum={auto_vacuum};"))?;
    let tx = source_conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    tx.query_row("SELECT sqlcipher_export('encrypted')", [], |_| Ok(()))?;
    tx.execute_batch(&format!("PRAGMA encrypted.user_version={user_version}; PRAGMA encrypted.application_id={application_id};"))?;
    tx.commit()?;
    // SQLCipher exports virtual tables via sqlite_schema. Reattach before
    // querying them so SQLite reloads their schema (notably external FTS5).
    source_conn.execute_batch("DETACH DATABASE encrypted;")?;
    source_conn.execute(
        "ATTACH DATABASE ?1 AS encrypted KEY ?2",
        rusqlite::params![staging.to_string_lossy().as_ref(), raw_key.as_str()],
    )?;
    let tx = source_conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    verify_export(&tx)?;
    tx.commit()?;
    source_conn.execute_batch("DETACH DATABASE encrypted;")?;
    drop(source_conn);

    {
        let mut conn = open_encrypted_database(&staging, master_key.as_bytes(), false)?;
        if !check_encrypted_integrity(&conn)? {
            return Err(migration_error("加密暂存库完整性校验失败"));
        }
        run_migrations(&mut conn)?;
        let fk_errors: i64 =
            conn.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })?;
        if fk_errors != 0 {
            return Err(migration_error("数据库外键关系校验失败"));
        }
        if !check_encrypted_integrity(&conn)? {
            return Err(migration_error("升级后加密库校验失败"));
        }
        let (busy, _, _): (i64, i64, i64) =
            conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?;
        if busy != 0 {
            return Err(migration_error("加密暂存库 WAL 无法合并"));
        }
    }
    fs::OpenOptions::new()
        .write(true)
        .open(&staging)?
        .sync_all()?;
    write_atomic(&pending, &serde_json::to_vec_pretty(&envelope)?)?;
    marker.db_sha256 = Some(compute_file_sha256(&staging)?);
    marker.envelope_sha256 = Some(compute_file_sha256(&pending)?);
    // A portable encrypted recovery pair is retained before retiring plaintext.
    let recovery_dir = data_dir
        .join("migration-recovery")
        .join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&recovery_dir)?;
    let recovery_db = recovery_dir.join("archive.db");
    fs::copy(&staging, &recovery_db)?;
    fs::OpenOptions::new()
        .write(true)
        .open(&recovery_db)?
        .sync_all()?;
    envelope
        .save_atomic(&recovery_dir)
        .map_err(AppError::Security)?;
    if !matches_hash(&recovery_db, &marker.db_sha256)? {
        return Err(migration_error("加密恢复副本校验失败"));
    }
    {
        let conn = open_encrypted_database(&recovery_db, master_key.as_bytes(), false)?;
        if !check_encrypted_integrity(&conn)? {
            return Err(migration_error("加密恢复副本无法验证"));
        }
    }
    marker.stage = "staged_verified".into();
    marker.save(data_dir)?;
    install_verified_pair(data_dir, &mut marker, true)?;
    info!("SQLCipher 迁移校验完成；已保留明文原件与加密恢复副本，未自动删除原始数据");
    Ok((envelope, master_key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupt_marker_and_unproven_files_are_preserved() {
        let root =
            std::env::temp_dir().join(format!("browsory_fail_closed_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(MigrationStateMarker::marker_path(&root), b"{broken").unwrap();
        fs::write(root.join("archive.staging.db"), b"sole remaining copy").unwrap();
        assert!(recover_from_interrupted_migration(&root).is_err());
        let vault = crate::security::vault::VaultManager::new(root.clone());
        assert!(matches!(
            vault.status(),
            crate::security::vault::VaultStatus::Error(_)
        ));
        assert_eq!(
            fs::read(root.join("archive.staging.db")).unwrap(),
            b"sole remaining copy"
        );
        assert_eq!(
            fs::read(MigrationStateMarker::marker_path(&root)).unwrap(),
            b"{broken"
        );
        assert!(!root.join("archive.db").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recorded_cutover_windows_recover_and_install_errors_keep_success_result() {
        use crate::security::envelope::{KdfParameters, MIN_MEM_COST_KB};
        let params = KdfParameters {
            mem_cost_kb: MIN_MEM_COST_KB,
            time_cost: 1,
            parallelism: 1,
        };
        let (envelope, _, key) =
            create_vault_envelope("Crash-recovery-passphrase", Some(params), 0).unwrap();
        // 0: before retiring plaintext, 1: original renamed, 2: encrypted DB
        // installed, 3: envelope installed, 4: verified new-vault creation,
        // 5: an in-process error after installing DB must finish and return Ok.
        for window in 0..6 {
            let root =
                std::env::temp_dir().join(format!("browsory_cutover_{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&root).unwrap();
            let source = root.join("archive.db");
            let staging = root.join("archive.staging.db");
            if window != 4 {
                let plain = Connection::open(&source).unwrap();
                plain.execute_batch("CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES('original');").unwrap();
            }
            {
                let encrypted = open_encrypted_database(&staging, key.as_bytes(), true).unwrap();
                encrypted.execute_batch("CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES('encrypted');").unwrap();
                checkpoint_truncate(&encrypted).unwrap();
            }
            write_atomic(
                &root.join(PENDING_ENVELOPE),
                &serde_json::to_vec_pretty(&envelope).unwrap(),
            )
            .unwrap();
            let mut marker = MigrationStateMarker {
                stage: if window == 4 {
                    "creation_verified".into()
                } else {
                    "staged_verified".into()
                },
                pre_backup_path: "untrusted-path-must-never-be-used".into(),
                created_at: 0,
                db_sha256: Some(compute_file_sha256(&staging).unwrap()),
                envelope_sha256: Some(compute_file_sha256(&root.join(PENDING_ENVELOPE)).unwrap()),
            };
            marker.save(&root).unwrap();
            if window == 5 {
                let result = install_verified_pair_observed(&root, &mut marker, true, |_| {
                    Err(migration_error("injected post-install failure"))
                });
                assert!(result.is_ok(), "completed recovery must preserve the caller's success and recovery-key response");
            } else {
                if (1..=3).contains(&window) {
                    fs::rename(&source, root.join("archive.db.plaintext.bak")).unwrap();
                }
                if (2..=3).contains(&window) {
                    fs::rename(&staging, &source).unwrap();
                }
                if window == 3 {
                    fs::rename(
                        root.join(PENDING_ENVELOPE),
                        VaultEnvelope::envelope_path(&root),
                    )
                    .unwrap();
                }
                assert!(recover_from_interrupted_migration(&root).unwrap());
            }
            assert!(!MigrationStateMarker::marker_path(&root).exists());
            if window < 2 {
                assert!(is_plain_sqlite_database(&source));
                assert!(!VaultEnvelope::envelope_path(&root).exists());
                assert_eq!(
                    fs::read_dir(root.join("migration-interrupted"))
                        .unwrap()
                        .count(),
                    1
                );
            } else {
                let conn = open_encrypted_database(&source, key.as_bytes(), false).unwrap();
                let value: String = conn
                    .query_row("SELECT value FROM sentinel", [], |r| r.get(0))
                    .unwrap();
                assert_eq!(value, "encrypted");
            }
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn interrupted_initial_creation_returns_to_setup_without_deleting_staging() {
        let root =
            std::env::temp_dir().join(format!("browsory_new_crash_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("archive.staging.db"), b"partial encrypted file").unwrap();
        MigrationStateMarker {
            stage: "creating".into(),
            pre_backup_path: String::new(),
            created_at: 0,
            db_sha256: None,
            envelope_sha256: None,
        }
        .save(&root)
        .unwrap();
        let vault = crate::security::vault::VaultManager::new(root.clone());
        assert_eq!(
            vault.status(),
            crate::security::vault::VaultStatus::Unconfigured
        );
        assert!(!root.join("archive.db").exists());
        assert!(root.join("migration-interrupted").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn test_sqlite_header_magic_detection() {
        let temp_dir =
            std::env::temp_dir().join(format!("browsory_magic_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("plain.db");

        // Create plaintext SQLite
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch("CREATE TABLE t (id INT);").unwrap();
        }

        assert!(is_plain_sqlite_database(&db_path));
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn export_comparison_handles_primary_keys_null_duplicates_and_binary_text() {
        for schema in [
            "id INTEGER PRIMARY KEY, value BLOB",
            "id TEXT NOT NULL PRIMARY KEY COLLATE NOCASE,value BLOB",
            "id TEXT PRIMARY KEY,value BLOB",
            "id INTEGER PRIMARY KEY DESC,value BLOB",
        ] {
            let conn = Connection::open_in_memory().unwrap();
            conn.execute_batch(&format!("ATTACH ':memory:' AS encrypted;CREATE TABLE main.t({schema});CREATE TABLE encrypted.t({schema});")).unwrap();
            conn.execute_batch("INSERT INTO main.t VALUES(1,X'0001FF'); INSERT INTO encrypted.t SELECT * FROM main.t;").unwrap();
            verify_export(&conn).unwrap();
            conn.execute_batch("UPDATE encrypted.t SET value=X'0001FE';")
                .unwrap();
            assert!(verify_export(&conn).is_err());
        }
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("ATTACH ':memory:' AS encrypted;CREATE TABLE main.t(id TEXT PRIMARY KEY,value TEXT);CREATE TABLE encrypted.t(id TEXT PRIMARY KEY,value TEXT);INSERT INTO main.t VALUES(NULL,'a'),(NULL,'b');INSERT INTO encrypted.t VALUES(NULL,'a'),(NULL,'a');").unwrap();
        assert!(
            verify_export(&conn).is_err(),
            "nullable keys must retain duplicate counts"
        );
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("ATTACH ':memory:' AS encrypted;CREATE TABLE main.t(id TEXT NOT NULL PRIMARY KEY COLLATE NOCASE,value TEXT COLLATE NOCASE);CREATE TABLE encrypted.t(id TEXT NOT NULL PRIMARY KEY COLLATE NOCASE,value TEXT COLLATE NOCASE);INSERT INTO main.t VALUES('Key','Case');INSERT INTO encrypted.t VALUES('key','case');").unwrap();
        assert!(
            verify_export(&conn).is_err(),
            "key and text bytes must match even under NOCASE collation"
        );
    }

    #[test]
    fn insufficient_migration_space_leaves_source_and_wal_untouched_and_retries() {
        let root =
            std::env::temp_dir().join(format!("browsory_space_preflight_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("archive.db");
        let wal = root.join("archive.db-wal");
        let mut writer = Connection::open(&source).unwrap();
        writer
            .query_row("PRAGMA journal_mode=WAL", [], |r| r.get::<_, String>(0))
            .unwrap();
        writer
            .query_row("PRAGMA wal_autocheckpoint=0", [], |r| r.get::<_, i64>(0))
            .unwrap();
        run_migrations(&mut writer).unwrap();
        writer.execute_batch("CREATE TABLE preserved_space_test(value TEXT); INSERT INTO preserved_space_test VALUES('retry preserved');").unwrap();
        let source_bytes = fs::read(&source).unwrap();
        let wal_bytes = fs::read(&wal).unwrap();
        let required = migration_requirement(
            fs::metadata(&source).unwrap().len(),
            fs::metadata(&wal).unwrap().len(),
        )
        .unwrap();

        let password = "Space-Preflight-Retry-Passphrase-2026!";
        let insufficient = migrate_with_space_preflight(&root, password, |directory, original| {
            migration_space_preflight_with(directory, original, |_| Ok(required.saturating_sub(1)))
        });
        assert!(insufficient.is_err());
        assert_eq!(fs::read(&source).unwrap(), source_bytes);
        assert_eq!(fs::read(&wal).unwrap(), wal_bytes);
        assert!(!MigrationStateMarker::marker_path(&root).exists());
        assert!(!root.join("archive.staging.db").exists());

        assert!(!VaultEnvelope::envelope_path(&root).exists());
        assert!(!root.join("migration-recovery").exists());
        assert!(!root.join("archive.db.plaintext.bak").exists());
        drop(writer);
        // Retry the real migration entry point, including KDF/export/switch.
        let (envelope, _) = migrate_plaintext_to_sqlcipher(&root, password).unwrap();
        let key = crate::security::envelope::unwrap_master_key_with_password(&envelope, password)
            .unwrap();
        let encrypted = open_encrypted_database(&source, key.as_bytes(), false).unwrap();
        let value: String = encrypted
            .query_row("SELECT value FROM preserved_space_test", [], |r| r.get(0))
            .unwrap();
        assert_eq!(value, "retry preserved");
        assert!(root.join("archive.db.plaintext.bak").exists());
        drop(encrypted);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn test_migrate_database_safely_roundtrip() {
        let temp_dir = std::env::temp_dir().join(format!("browsory_mig_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let source_path = temp_dir.join("archive.db");

        // Create initial plain database with realistic schema and data
        {
            let mut conn = Connection::open(&source_path).unwrap();
            run_migrations(&mut conn).unwrap();

            conn.execute(
                "INSERT INTO sources (browser, profile, source_type, history_path) VALUES ('chrome', 'Default', 'sqlite', 'fake');",
                [],
            ).unwrap();
            conn.execute(
                "INSERT INTO urls (url, title, domain) VALUES ('https://github.com', 'GitHub', 'github.com');",
                [],
            ).unwrap();
            conn.execute(
                "INSERT INTO visits (source_id, url_id, source_visit_id, visit_time, event_hash, imported_at) VALUES (1, 1, 100, 1700000000, 'hash1', 1700000000);",
                [],
            ).unwrap();
            conn.execute_batch(
                "CREATE TABLE extension_data(id INTEGER PRIMARY KEY AUTOINCREMENT, payload BLOB, label TEXT);
                 CREATE TABLE extension_audit(value TEXT);
                 CREATE UNIQUE INDEX extension_label ON extension_data(label);
                 CREATE VIEW extension_view AS SELECT label FROM extension_data;
                 CREATE TRIGGER extension_update AFTER UPDATE ON extension_data BEGIN INSERT INTO extension_audit VALUES(new.label); END;
                 INSERT INTO extension_data(id,payload,label) VALUES(42, x'0011ff', 'preserved');
                 INSERT INTO extension_data(id,payload,label) VALUES(99, x'22', 'removed');
                 DELETE FROM extension_data WHERE id=99;
                 INSERT INTO settings(key,value) VALUES('pin_enabled','true');
                 PRAGMA user_version=321;
                 PRAGMA application_id=12345;"
            ).unwrap();
        }

        assert!(is_plain_sqlite_database(&source_path));

        // Perform migration
        let password = "Vault-Migration-Strong-Passphrase-2026!";
        let (env, rec_key) =
            migrate_plaintext_to_sqlcipher(&temp_dir, password).expect("Migration succeeds");

        // After migration:
        // 1. archive.db must NOT be plain SQLite anymore!
        assert!(!is_plain_sqlite_database(&source_path));

        // 2. Plaintext backup must exist
        assert!(temp_dir.join("archive.db.plaintext.bak").exists());

        // 3. Envelope must exist
        assert!(VaultEnvelope::envelope_path(&temp_dir).exists());

        // 4. Decrypting with master key works
        let master_key =
            crate::security::envelope::unwrap_master_key_with_password(&env, password).unwrap();
        let enc_conn = open_encrypted_database(&source_path, &master_key.0, false).unwrap();
        assert!(check_encrypted_integrity(&enc_conn).unwrap());

        // 5. Data verified intact
        let url_count: i64 = enc_conn
            .query_row("SELECT count(*) FROM urls;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(url_count, 1);

        let visit_count: i64 = enc_conn
            .query_row("SELECT count(*) FROM visits;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(visit_count, 1);
        let payload: Vec<u8> = enc_conn
            .query_row("SELECT payload FROM extension_data WHERE id=42", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(payload, [0x00, 0x11, 0xff]);
        let pin: String = enc_conn
            .query_row(
                "SELECT value FROM settings WHERE key='pin_enabled'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(pin, "true");
        assert_eq!(
            enc_conn
                .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            321
        );
        assert_eq!(
            enc_conn
                .pragma_query_value(None, "application_id", |r| r.get::<_, i64>(0))
                .unwrap(),
            12345
        );
        enc_conn
            .execute("UPDATE extension_data SET label='updated' WHERE id=42", [])
            .unwrap();
        let audit: String = enc_conn
            .query_row("SELECT value FROM extension_audit", [], |r| r.get(0))
            .unwrap();
        assert_eq!(audit, "updated");
        enc_conn
            .execute("INSERT INTO extension_data(label) VALUES('new')", [])
            .unwrap();
        assert_eq!(enc_conn.last_insert_rowid(), 100);
        assert!(!temp_dir.join("backups").exists());
        assert_eq!(
            fs::read_dir(temp_dir.join("migration-recovery"))
                .unwrap()
                .count(),
            1
        );
        assert!(!recover_from_interrupted_migration(&temp_dir).unwrap());

        // 6. FTS5 search query works on migrated data
        let fts_count: i64 = enc_conn
            .query_row(
                "SELECT count(*) FROM urls_fts WHERE urls_fts MATCH 'GitHub';",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fts_count, 1);

        // 7. Recovery key is valid
        let rec_master =
            crate::security::envelope::unwrap_master_key_with_recovery(&env, &rec_key).unwrap();
        assert_eq!(master_key.0, rec_master.0);

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[cfg(windows)]
    #[test]
    fn passive_migration_keeps_plaintext_recovery_copy_until_explicit_retirement() {
        let root = std::env::temp_dir().join(format!(
            "browsory_passive_migration_{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("archive.db");
        {
            let conn = Connection::open(&source).unwrap();
            conn.execute_batch(
                "CREATE TABLE preserved(value TEXT NOT NULL);
                 INSERT INTO preserved VALUES('recovery fixture');",
            )
            .unwrap();
        }

        let (envelope, key) = migrate_plaintext_to_sqlcipher_passive(&root).unwrap();
        assert!(!envelope.recovery_key_confirmed);
        assert!(root.join("archive.db.plaintext.bak").is_file());
        let encrypted = open_encrypted_database(&source, key.as_bytes(), false).unwrap();
        let value: String = encrypted
            .query_row("SELECT value FROM preserved", [], |row| row.get(0))
            .unwrap();
        assert_eq!(value, "recovery fixture");
        drop(encrypted);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn test_recover_from_interrupted_migration() {
        let temp_dir =
            std::env::temp_dir().join(format!("browsory_crash_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let source_path = temp_dir.join("archive.db");
        let bak_path = temp_dir.join("archive.db.plaintext.bak");
        let staging_path = temp_dir.join("archive.staging.db");

        // 1. Setup simulated crash state: archive.db was renamed to bak, staging exists
        {
            let mut conn = Connection::open(&bak_path).unwrap();
            run_migrations(&mut conn).unwrap();
        }
        fs::write(&staging_path, b"incomplete staging db bytes").unwrap();

        let marker = MigrationStateMarker {
            stage: "staged_verified".into(),
            pre_backup_path: "fake_pre_backup".into(),
            db_sha256: None,
            envelope_sha256: None,
            created_at: 1700000000,
        };
        marker.save(&temp_dir).unwrap();

        assert!(!source_path.exists());
        assert!(staging_path.exists());
        assert!(MigrationStateMarker::marker_path(&temp_dir).exists());

        // 2. Perform recovery
        let recovered = recover_from_interrupted_migration(&temp_dir).expect("Recovery succeeds");
        assert!(recovered);

        // 3. Verify healed state
        assert!(source_path.exists(), "Source archive.db must be restored");
        assert!(
            is_plain_sqlite_database(&source_path),
            "Restored db must be valid plaintext SQLite"
        );
        assert!(!staging_path.exists(), "Staging db must be cleaned");
        assert!(
            !MigrationStateMarker::marker_path(&temp_dir).exists(),
            "Marker must be cleaned"
        );

        // 4. VaultManager state refresh yields LegacyPlaintext, not Error
        let vault = crate::security::vault::VaultManager::new(temp_dir.clone());
        assert_eq!(
            vault.status(),
            crate::security::vault::VaultStatus::LegacyPlaintext
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
