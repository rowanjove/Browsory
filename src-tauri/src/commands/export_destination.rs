//! User exports must not replace the database, its recovery state, or managed files.
use crate::error::{AppError, AppResult};
use std::path::Path;

pub(crate) fn validate(app_dir: &Path, target: &Path) -> AppResult<()> {
    let root = app_dir.canonicalize()?;
    let resolved = if target.exists() {
        target.canonicalize()?
    } else {
        let parent = target
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        parent.canonicalize()?.join(
            target
                .file_name()
                .ok_or_else(|| AppError::Parse("导出文件名不能为空".into()))?,
        )
    };
    let Ok(relative) = resolved.strip_prefix(root) else {
        return Ok(());
    };
    let first = relative
        .components()
        .next()
        .map(|c| c.as_os_str().to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let protected = [
        "archive",
        "archive-retired",
        "backups",
        "secrets",
        "logs",
        "vault.restore",
        "snapshot-work",
        "backup-work",
        "temp",
        "plaintext-retired",
        "migration-recovery",
        "migration-interrupted",
        "restore-recovery",
    ];
    if protected.contains(&first.as_str())
        || first.starts_with("archive.db")
        || first.starts_with("archive.staging.db")
        || first.starts_with("archive.legacy.staging.db")
        || first.starts_with("vault.envelope")
        || first.starts_with("migration.")
        || first.starts_with("restore.")
        || first.starts_with("auto_sync_state")
        || first == "vault.instance.lock"
        || first == "sync_secret.key"
        || first == "device_id.txt"
        || first == "portable.txt"
        || first == "portable"
    {
        return Err(AppError::Security(
            "导出位置属于保险库管理文件，请选择其他文件或目录".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuses_managed_paths_but_preserves_explicit_user_exports() {
        let root =
            std::env::temp_dir().join(format!("browsory_export_target_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        for name in [
            "archive.db",
            "archive.db-wal",
            "vault.envelope.json",
            "migration.state.json",
            "restore.state.json",
            "auto_sync_state.json",
            "sync_secret.key",
            "device_id.txt",
            "portable.txt",
            "portable",
        ] {
            let target = root.join(name);
            assert!(validate(&root, &target).is_err());
            std::fs::write(&target, b"managed fixture").unwrap();
            assert!(validate(&root, &target).is_err());
            assert_eq!(std::fs::read(target).unwrap(), b"managed fixture");
        }
        let backups = root.join("backups");
        std::fs::create_dir(&backups).unwrap();
        assert!(validate(&root, &backups.join("backup.browsory-backup")).is_err());
        for directory in [
            "plaintext-retired",
            "migration-recovery",
            "migration-interrupted",
            "restore-recovery",
        ] {
            let recovery = root.join(directory).join("fixture");
            std::fs::create_dir_all(&recovery).unwrap();
            for name in ["archive.db", "vault.envelope.json", "new-export.json"] {
                let target = recovery.join(name);
                assert!(validate(&root, &target).is_err());
                std::fs::write(&target, b"retained recovery material").unwrap();
                assert!(validate(&root, &target).is_err());
                assert_eq!(
                    std::fs::read(&target).unwrap(),
                    b"retained recovery material"
                );
            }
        }
        assert!(validate(&root, &root.join("archive.staging.db")).is_err());
        assert!(validate(&root, &root.join("archive.legacy.staging.db")).is_err());
        assert!(validate(&root, &root.join("my-history.csv")).is_ok());
        assert!(validate(&root, &root.parent().unwrap().join("synthetic-export.csv")).is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }
}
