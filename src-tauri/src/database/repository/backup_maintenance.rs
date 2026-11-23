use rusqlite::{Connection, OptionalExtension};
use std::fs;
use std::path::{Component, Path, PathBuf};

pub(crate) const ARCHIVE_BACKUP_RETENTION: usize = 15;

#[derive(Debug, serde::Deserialize)]
struct OrphanBackupMarker {
    file_name: String,
    #[serde(default)]
    created_at: Option<i64>,
}

/// Discovers published backup packages whose DB registration failed earlier
/// (marked by `.{file_name}.orphan.json`), verifies their bounded ZIP/manifest
/// structure, re-registers missing rows in `backups`, and removes resolved
/// marker files. Invalid markers or packages never delete the backup file.
pub(crate) fn reconcile_orphan_backups(
    conn: &Connection,
    backups_dir: &Path,
) -> Result<usize, String> {
    if !backups_dir.is_dir() {
        return Ok(0);
    }
    let entries = fs::read_dir(backups_dir).map_err(|error| error.to_string())?;
    let mut recovered = 0usize;
    let mut failures = Vec::new();

    for entry in entries.flatten() {
        let marker_path = entry.path();
        if !marker_path.is_file() {
            continue;
        }
        let Some(marker_name) = marker_path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(expected_backup_name) = marker_name
            .strip_prefix('.')
            .and_then(|rest| rest.strip_suffix(".orphan.json"))
        else {
            continue;
        };
        if !is_safe_backup_name(expected_backup_name) {
            failures.push(format!("非法孤儿标记文件名: {marker_name}"));
            continue;
        }
        let marker_bytes = match fs::read(&marker_path) {
            Ok(bytes) => bytes,
            Err(error) => {
                failures.push(format!("读取孤儿标记失败 {marker_name}: {error}"));
                continue;
            }
        };
        let marker: OrphanBackupMarker = match serde_json::from_slice(&marker_bytes) {
            Ok(parsed) => parsed,
            Err(error) => {
                failures.push(format!("解析孤儿标记失败 {marker_name}: {error}"));
                continue;
            }
        };
        if marker.file_name != expected_backup_name {
            failures.push(format!("孤儿标记文件名不一致: {marker_name}"));
            continue;
        }

        let package_path = backups_dir.join(expected_backup_name);
        if !package_path.is_file() {
            // Backup was removed externally; leave non-existent package unregistered
            // only if confirmed missing. Keep marker for visibility unless package
            // already exists in DB.
            let already_registered: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM backups WHERE file_name = ?1)",
                    [expected_backup_name],
                    |row| row.get(0),
                )
                .unwrap_or(false);
            if already_registered {
                let _ = fs::remove_file(&marker_path);
            } else {
                failures.push(format!("孤儿备份文件缺失: {expected_backup_name}"));
            }
            continue;
        }

        let manifest_created_at = match super::verify_backup_package_created_at(&package_path) {
            Ok(ts) => ts,
            Err(error) => {
                failures.push(format!(
                    "孤儿备份结构校验失败 {expected_backup_name}: {error}"
                ));
                continue;
            }
        };

        let existing: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM backups WHERE file_name = ?1)",
                [expected_backup_name],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !existing {
            let bytes = fs::metadata(&package_path)
                .map_err(|error| error.to_string())?
                .len();
            let created_at = marker.created_at.unwrap_or(manifest_created_at);
            conn.execute(
                "INSERT INTO backups (file_name, file_path, file_size_bytes, created_at, status, notes) \
                 VALUES (?1, ?2, ?3, ?4, 'success', ?5)",
                rusqlite::params![
                    expected_backup_name,
                    package_path.to_string_lossy().as_ref(),
                    bytes as i64,
                    created_at,
                    Some("自动发现并补录的加密备份")
                ],
            )
            .map_err(|error| error.to_string())?;
            recovered += 1;
        }
        if let Err(error) = fs::remove_file(&marker_path) {
            failures.push(format!("移除已补录标记失败 {marker_name}: {error}"));
        }
    }

    if failures.is_empty() {
        Ok(recovered)
    } else {
        Err(failures.join("; "))
    }
}

#[derive(Debug)]
struct BackupFileRecord {
    id: i64,
    file_name: String,
    file_path: String,
}

/// Keep the newest successful normal ZIP backups. Manual and automatic
/// backups share the archive_backup_ prefix and one retention budget.
pub(crate) fn rotate_archive_backups(conn: &Connection, backups_dir: &Path) -> Result<(), String> {
    let mut statement = conn
        .prepare(
            "SELECT id, file_name, file_path FROM backups \
             WHERE status = 'success' AND file_name GLOB 'archive_backup_*' \
             ORDER BY created_at DESC, id DESC",
        )
        .map_err(|error| error.to_string())?;
    let records = statement
        .query_map([], |row| {
            Ok(BackupFileRecord {
                id: row.get(0)?,
                file_name: row.get(1)?,
                file_path: row.get(2)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;

    let mut failures = Vec::new();
    for record in records.iter().skip(ARCHIVE_BACKUP_RETENTION) {
        match remove_record_file(backups_dir, record) {
            Ok(()) => {
                if let Err(error) = conn.execute("DELETE FROM backups WHERE id = ?1", [record.id]) {
                    failures.push(format!(
                        "backup record {} could not be removed after file deletion: {}",
                        record.id, error
                    ));
                }
            }
            Err(error) => failures.push(format!("backup {} was retained: {}", record.id, error)),
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

/// Delete a selected backup file first, then its database row.
pub(crate) fn delete_backup_record(
    conn: &Connection,
    id: i64,
    backups_dir: &Path,
) -> Result<(), String> {
    let record: Option<BackupFileRecord> = conn
        .query_row(
            "SELECT id, file_name, file_path FROM backups WHERE id = ?1",
            [id],
            |row| {
                Ok(BackupFileRecord {
                    id: row.get(0)?,
                    file_name: row.get(1)?,
                    file_path: row.get(2)?,
                })
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;

    let Some(record) = record else {
        return Ok(());
    };
    remove_record_file(backups_dir, &record)?;
    conn.execute("DELETE FROM backups WHERE id = ?1", [record.id])
        .map_err(|error| format!("备份文件已删除，但备份记录删除失败: {}", error))?;
    Ok(())
}

fn remove_record_file(backups_dir: &Path, record: &BackupFileRecord) -> Result<(), String> {
    let path = validated_backup_path(backups_dir, &record.file_name, &record.file_path)?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("无法删除备份文件 {:?}: {}", path, error)),
    }
}

fn validated_backup_path(
    backups_dir: &Path,
    file_name: &str,
    recorded_path: &str,
) -> Result<PathBuf, String> {
    if !is_safe_backup_name(file_name) {
        return Err("备份文件名不在受支持的备份目录边界内".to_string());
    }
    // A missing backups directory means the file is already gone. Keep a
    // lexical absolute boundary so the corresponding row can still be
    // removed safely, while permission errors remain visible at remove_file.
    let root = fs::canonicalize(backups_dir).unwrap_or_else(|_| absolute_path(backups_dir));
    let candidate = root.join(file_name);
    let candidate_cmp = comparable_path(&candidate);
    let recorded_cmp = comparable_path(Path::new(recorded_path));
    let root_cmp = comparable_path(&root);
    if !candidate_cmp.starts_with(&root_cmp) {
        return Err("备份文件路径越过备份目录边界".to_string());
    }
    if candidate_cmp != recorded_cmp {
        return Err("备份记录路径与受控文件名不一致".to_string());
    }
    Ok(candidate)
}

fn is_safe_backup_name(file_name: &str) -> bool {
    !file_name.is_empty()
        && file_name.ends_with(".browsory-backup")
        && Path::new(file_name)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn comparable_path(path: &Path) -> String {
    let absolute = fs::canonicalize(path).unwrap_or_else(|_| absolute_path(path));
    absolute.to_string_lossy().replace('\\', "/").to_lowercase()
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn setup() -> (Connection, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("browsory_backup_maintenance_{}", Uuid::new_v4()));
        let backups = root.join("backups");
        fs::create_dir_all(&backups).unwrap();
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE backups(
                id INTEGER PRIMARY KEY,
                file_name TEXT NOT NULL,
                file_path TEXT NOT NULL,
                file_size_bytes INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                status TEXT NOT NULL,
                notes TEXT
            );",
        )
        .unwrap();
        (conn, root)
    }

    fn insert_backup(conn: &Connection, backups: &Path, id: i64, created_at: i64) -> PathBuf {
        let file_name = format!("archive_backup_{}_{}.browsory-backup", created_at, id);
        let path = backups.join(&file_name);
        fs::write(&path, b"backup").unwrap();
        conn.execute(
            "INSERT INTO backups(id,file_name,file_path,file_size_bytes,created_at,status) VALUES(?1,?2,?3,6,?4,'success')",
            rusqlite::params![id, file_name, path.to_string_lossy(), created_at],
        )
        .unwrap();
        path
    }

    #[test]
    fn retains_latest_fifteen_manual_and_automatic_backups() {
        let (conn, root) = setup();
        let backups = root.join("backups");
        for id in 1..=16 {
            insert_backup(&conn, &backups, id, id);
        }

        rotate_archive_backups(&conn, &backups).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM backups", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 15);
        assert!(!backups.join("archive_backup_1_1.browsory-backup").exists());
        assert!(backups
            .join("archive_backup_16_16.browsory-backup")
            .exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn deletion_failure_keeps_record_and_can_be_retried() {
        let (conn, root) = setup();
        let backups = root.join("backups");
        for id in 1..=16 {
            insert_backup(&conn, &backups, id, id);
        }
        let oldest = backups.join("archive_backup_1_1.browsory-backup");
        fs::remove_file(&oldest).unwrap();
        fs::create_dir(&oldest).unwrap();

        assert!(rotate_archive_backups(&conn, &backups).is_err());
        let retained: i64 = conn
            .query_row("SELECT COUNT(*) FROM backups WHERE id = 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(retained, 1);

        fs::remove_dir(&oldest).unwrap();
        fs::write(&oldest, b"backup").unwrap();
        rotate_archive_backups(&conn, &backups).unwrap();
        let removed: i64 = conn
            .query_row("SELECT COUNT(*) FROM backups WHERE id = 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(removed, 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malicious_path_is_rejected_without_deleting_outside_file() {
        let (conn, root) = setup();
        let backups = root.join("backups");
        for id in 1..=15 {
            insert_backup(&conn, &backups, id, id + 1);
        }
        let outside = root.join("outside.browsory-backup");
        fs::write(&outside, b"do not delete").unwrap();
        conn.execute(
            "INSERT INTO backups(id,file_name,file_path,file_size_bytes,created_at,status) VALUES(99,'archive_backup_foo/../../outside.browsory-backup',?1,12,1,'success')",
            [&outside.to_string_lossy().to_string()],
        )
        .unwrap();

        assert!(rotate_archive_backups(&conn, &backups).is_err());
        assert!(delete_backup_record(&conn, 99, &backups).is_err());
        assert!(outside.exists());
        assert_eq!(fs::read(&outside).unwrap(), b"do not delete");
        let retained: i64 = conn
            .query_row("SELECT COUNT(*) FROM backups WHERE id = 99", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(retained, 1);
        fs::remove_dir_all(root).unwrap();
    }
}
