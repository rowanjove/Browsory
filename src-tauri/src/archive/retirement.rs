//! Explicit, reauthenticated retirement of one retained legacy source page.
//! No startup deletion. A verified encrypted recovery pair precedes any unlink.
use super::{
    legacy::{manifest, read_page, read_stored_page, LegacyFile},
    validate_uuid,
};
use crate::{
    database::{
        space::{protected_snapshot_requirement, require_available_space},
        sqlcipher::{check_encrypted_integrity, open_encrypted_database},
    },
    error::{AppError, AppResult},
    security::{
        atomic_file::write_atomic,
        backup::compute_file_sha256,
        envelope::{
            unwrap_master_key_with_dpapi, unwrap_master_key_with_password, MasterKey, VaultEnvelope,
        },
        fs_boundary::{plain_metadata, resolve_scoped_path},
    },
};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

const CONFIRM: &str = "移除旧网页源目录";
const MAX_PAGES: usize = 4096;
#[derive(Debug, Serialize)]
pub struct SourcePage {
    pub page_uuid: String,
    pub file_count: usize,
    pub total_bytes: u64,
    pub can_retire: bool,
    pub resuming: bool,
    pub reason: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct RetiredPage {
    pub page_uuid: String,
    pub recovery_relative_path: String,
}
#[derive(Serialize, Deserialize)]
struct Receipt {
    version: u32,
    page_uuid: String,
    manifest_sha256: String,
    recovery_id: String,
    db_sha256: String,
    envelope_sha256: String,
    complete: bool,
}
fn invalid(message: &str) -> AppError {
    AppError::Security(message.into())
}
fn relative(uuid: &str) -> PathBuf {
    Path::new("archive/pages").join(uuid)
}
fn retirement_base(uuid: &str) -> PathBuf {
    Path::new("archive-retired").join(uuid)
}
fn ensure_directory(root: &Path, path: &Path) -> AppResult<PathBuf> {
    let mut current = PathBuf::new();
    for part in path.components() {
        if !matches!(part, std::path::Component::Normal(_)) {
            return Err(invalid("退役目录边界无效"));
        }
        current.push(part.as_os_str());
        if resolve_scoped_path(root, &current)?.is_none() {
            fs::create_dir(root.join(&current))?;
        }
        let resolved =
            resolve_scoped_path(root, &current)?.ok_or_else(|| invalid("退役目录消失"))?;
        if !plain_metadata(&resolved)?.is_some_and(|m| m.is_dir()) {
            return Err(invalid("退役路径不是普通目录"));
        }
    }
    resolve_scoped_path(root, path)?.ok_or_else(|| invalid("退役目录缺失"))
}
fn receipt(root: &Path, uuid: &str) -> AppResult<Option<Receipt>> {
    let bytes = crate::security::fs_boundary::read_scoped_file(
        root,
        &retirement_base(uuid).join("receipt.json"),
        16 * 1024,
    )?;
    let Some(bytes) = bytes else { return Ok(None) };
    let receipt: Receipt = serde_json::from_slice(&bytes)?;
    validate_uuid(&receipt.recovery_id)?;
    if receipt.version != 1 || receipt.page_uuid != uuid {
        return Err(invalid("旧网页退役记录无效，保留现场"));
    }
    Ok(Some(receipt))
}
fn verify_subset(source: &[LegacyFile], expected: &[LegacyFile]) -> AppResult<()> {
    for file in source {
        if !expected.iter().any(|stored| {
            stored.name == file.name
                && stored.sha256 == file.sha256
                && stored.bytes.len() == file.bytes.len()
        }) {
            return Err(invalid("旧网页剩余源文件与加密副本不符，保留全部材料"));
        }
    }
    Ok(())
}
pub fn inspect(conn: &Connection, root: &Path) -> AppResult<Vec<SourcePage>> {
    inspect_page(conn, root, 0, 20, || Ok(()))
}

pub fn inspect_page<F: Fn() -> AppResult<()>>(
    conn: &Connection,
    root: &Path,
    offset: usize,
    limit: usize,
    check: F,
) -> AppResult<Vec<SourcePage>> {
    let Some(base) = resolve_scoped_path(root, Path::new("archive/pages"))? else {
        return Ok(Vec::new());
    };
    let mut names = Vec::new();
    for (index, entry) in fs::read_dir(base)?.enumerate() {
        if index >= MAX_PAGES {
            return Err(invalid("旧网页源目录数量超过4096，请分批处理"));
        }
        let entry = entry?;
        let Some(uuid) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if validate_uuid(&uuid).is_err() {
            continue;
        }
        names.push(uuid);
    }
    names.sort();
    let mut pages = Vec::new();
    for uuid in names
        .into_iter()
        .skip(offset.min(MAX_PAGES))
        .take(limit.clamp(1, 20))
    {
        check()?;
        let checked = (|| -> AppResult<(usize, u64, bool)> {
            let source = read_page(root, &relative(&uuid), &check)?;
            let expected = read_stored_page(conn, &uuid)?;
            let previous = receipt(root, &uuid)?;
            let resuming = previous.as_ref().is_some_and(|r| !r.complete);
            if let Some(previous) = previous {
                if previous.manifest_sha256 != manifest(&expected) {
                    return Err(invalid("退役记录与当前加密副本不符"));
                }
            }
            if resuming {
                verify_subset(&source, &expected)?;
            } else if manifest(&source) != manifest(&expected) {
                return Err(invalid("源目录尚未完整迁移或已变化"));
            }
            Ok((
                source.len(),
                source.iter().map(|f| f.bytes.len() as u64).sum(),
                resuming,
            ))
        })();
        let (file_count, total_bytes, resuming, reason) = match checked {
            Ok((count, bytes, resuming)) => (count, bytes, resuming, None),
            Err(_) => (
                0,
                0,
                false,
                Some("源文件、加密副本或退役记录尚未验证；保留原目录。".into()),
            ),
        };
        pages.push(SourcePage {
            page_uuid: uuid,
            file_count,
            total_bytes,
            can_retire: reason.is_none(),
            resuming,
            reason,
        });
    }
    pages.sort_by(|a, b| a.page_uuid.cmp(&b.page_uuid));
    Ok(pages)
}
pub fn retire<F: Fn() -> AppResult<()>>(
    conn: &Connection,
    root: &Path,
    key: &MasterKey,
    envelope: &VaultEnvelope,
    request: (&str, &str, &str),
    check: F,
) -> AppResult<RetiredPage> {
    retire_observed(conn, root, key, envelope, request, check, || Ok(()))
}
fn retire_observed<F: Fn() -> AppResult<()>, O: FnOnce() -> AppResult<()>>(
    conn: &Connection,
    root: &Path,
    key: &MasterKey,
    envelope: &VaultEnvelope,
    request: (&str, &str, &str),
    check: F,
    observe: O,
) -> AppResult<RetiredPage> {
    let (uuid, password, confirmation) = request;
    validate_uuid(uuid)?;
    if confirmation != CONFIRM {
        return Err(invalid("请输入“移除旧网页源目录”确认本次操作"));
    }
    if !envelope.recovery_key_confirmed {
        return Err(invalid("请先保存并确认恢复密钥"));
    }
    let authenticated = if envelope.has_dpapi_wrap() && password.trim().is_empty() {
        unwrap_master_key_with_dpapi(envelope).map_err(AppError::Security)?
    } else {
        unwrap_master_key_with_password(envelope, password).map_err(AppError::Security)?
    };
    if authenticated.as_bytes() != key.as_bytes() {
        return Err(invalid("口令与当前保险库不匹配"));
    }
    check()?;
    for name in ["migration.state.json", "restore.state.json"] {
        if plain_metadata(&root.join(name))?.is_some() {
            return Err(invalid("存在未完成迁移/恢复，保留旧网页源目录"));
        }
    }
    let expected = read_stored_page(conn, uuid)?;
    let digest = manifest(&expected);
    let source = read_page(root, &relative(uuid), &check)?;
    let previous = receipt(root, uuid)?;
    if let Some(previous) = previous {
        if previous.manifest_sha256 != digest {
            return Err(invalid("旧网页退役状态或加密副本已变化"));
        }
        if previous.complete {
            if source.is_empty() || manifest(&source) != digest {
                return Err(invalid("重新出现的旧网页目录与完整加密副本不符"));
            }
        } else {
            // A resumed partial removal is only authorized by a still-valid full pair.
            let recovery = retirement_base(uuid).join(previous.recovery_id);
            let db = resolve_scoped_path(root, &recovery.join("archive.db"))?
                .ok_or_else(|| invalid("原退役恢复副本缺失"))?;
            let env = resolve_scoped_path(root, &recovery.join("vault.envelope.json"))?
                .ok_or_else(|| invalid("原退役恢复信封缺失"))?;
            if compute_file_sha256(&db)? != previous.db_sha256
                || compute_file_sha256(&env)? != previous.envelope_sha256
            {
                return Err(invalid("原退役恢复配对校验失败"));
            }
            let recovered = open_encrypted_database(&db, key.as_bytes(), false)?;
            if !check_encrypted_integrity(&recovered)?
                || manifest(&read_stored_page(&recovered, uuid)?) != digest
            {
                return Err(invalid("原退役恢复副本内容无效"));
            }
            verify_subset(&source, &expected)?;
        }
    } else if source.is_empty() || manifest(&source) != digest {
        return Err(invalid("旧网页源目录与加密迁移副本不一致"));
    }
    let pages: u64 = conn.pragma_query_value(None, "page_count", |r| r.get(0))?;
    let page_size: rusqlite::types::Value =
        conn.pragma_query_value(None, "page_size", |r| r.get(0))?;
    let page_size: u64 = match page_size {
        rusqlite::types::Value::Integer(size) if size > 0 => size as u64,
        rusqlite::types::Value::Text(size) => {
            size.parse().map_err(|_| invalid("保险库页面大小无效"))?
        }
        _ => return Err(invalid("保险库页面大小无效")),
    };
    if page_size == 0 {
        return Err(invalid("保险库页面大小为零"));
    }
    require_available_space(
        root,
        protected_snapshot_requirement(
            pages
                .checked_mul(page_size)
                .ok_or_else(|| invalid("数据库大小溢出"))?,
        )?,
    )?;
    let attempt = uuid::Uuid::new_v4().to_string();
    let archive = ensure_directory(root, &retirement_base(uuid).join(&attempt))?;
    let db = archive.join("archive.db");
    conn.execute("VACUUM main INTO ?1", [db.to_string_lossy().as_ref()])?;
    envelope.save_atomic(&archive).map_err(AppError::Security)?;
    {
        let saved = open_encrypted_database(&db, key.as_bytes(), false)?;
        if !check_encrypted_integrity(&saved)?
            || manifest(&read_stored_page(&saved, uuid)?) != digest
        {
            return Err(invalid("独立加密恢复副本验证失败，保留源目录"));
        }
        let busy: i64 = saved.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))?;
        if busy != 0 {
            return Err(invalid("退役恢复副本检查点未完成"));
        }
    }
    fs::OpenOptions::new().write(true).open(&db)?.sync_all()?;
    let mut state = Receipt {
        version: 1,
        page_uuid: uuid.into(),
        manifest_sha256: digest,
        recovery_id: attempt,
        db_sha256: compute_file_sha256(&db)?,
        envelope_sha256: compute_file_sha256(&archive.join("vault.envelope.json"))?,
        complete: false,
    };
    let state_path = root.join(retirement_base(uuid)).join("receipt.json");
    write_atomic(&state_path, &serde_json::to_vec_pretty(&state)?)?;
    observe()?;
    let refreshed = read_page(root, &relative(uuid), &check)?;
    if manifest(&refreshed) != manifest(&source) {
        return Err(invalid("源目录在验证期间发生变化，保留源目录和恢复配对"));
    }
    check()?;
    // This is the commit point. A later revocation cannot relabel success.
    let target =
        resolve_scoped_path(root, &relative(uuid))?.ok_or_else(|| invalid("旧网页源目录消失"))?;
    if !plain_metadata(&target)?.is_some_and(|m| m.is_dir()) {
        return Err(invalid("退役源不是普通目录"));
    }
    fs::remove_dir_all(&target)
        .map_err(|_| invalid("旧网页移除未全部完成；已保留验证后的恢复配对，可重新验证后继续"))?;
    state.complete = true;
    if write_atomic(&state_path, &serde_json::to_vec_pretty(&state)?).is_err() {
        tracing::warn!("旧网页源目录已移除；退役完成标记尚未更新，恢复配对保留");
    }
    Ok(RetiredPage {
        page_uuid: uuid.into(),
        recovery_relative_path: format!("archive-retired/{uuid}/{}", state.recovery_id),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::vault::VaultManager;
    fn fixture() -> (PathBuf, VaultManager, String) {
        let root =
            std::env::temp_dir().join(format!("browsory_retire_page_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let vault = VaultManager::unlocked_fixture(root.clone());
        let uuid = uuid::Uuid::new_v4().to_string();
        let path = root.join(relative(&uuid));
        fs::create_dir_all(path.join("assets")).unwrap();
        fs::write(path.join("content.md"), b"PRIVATE-PAGE-RETIRE-SENTINEL").unwrap();
        fs::write(path.join("assets/unknown.bin"), [0, 1, 255]).unwrap();
        vault.with_conn(|conn|{
            conn.execute_batch("INSERT INTO urls(id,url,title,domain) VALUES(1,'https://page.test','Page','page.test');")?;
            conn.execute("INSERT INTO offline_archives(url_id,page_uuid,relative_path,has_markdown,archived_at) VALUES(1,?1,?2,1,0)",rusqlite::params![uuid,format!("pages/{uuid}")])?;
            super::super::migrate_legacy_disk_archives(&root,conn)?;Ok(())
        }).unwrap();
        (root, vault, uuid)
    }
    #[test]
    fn explicit_page_retirement_preserves_verified_recovery_and_reading() {
        let (root, vault, uuid) = fixture();
        let key = vault.master_key().unwrap();
        let env = vault.envelope().unwrap();
        let unknown = root.join("archive/pages/unrecognized");
        fs::create_dir_all(&unknown).unwrap();
        fs::write(unknown.join("keep"), b"keep").unwrap();
        vault
            .with_conn(|conn| {
                let before = inspect(conn, &root)?;
                assert_eq!(before.len(), 1);
                assert!(before[0].can_retire);
                let result = retire(
                    conn,
                    &root,
                    &key,
                    &env,
                    (&uuid, "Synthetic-fixture-password", CONFIRM),
                    || Ok(()),
                )?;
                assert!(!root.join(relative(&uuid)).exists());
                assert_eq!(fs::read(unknown.join("keep"))?, b"keep");
                let detail = super::super::get_offline_archive(&root, conn, 1)?.unwrap();
                assert_eq!(
                    detail.markdown_content.as_deref(),
                    Some("PRIVATE-PAGE-RETIRE-SENTINEL")
                );
                let path = root.join(result.recovery_relative_path);
                let saved_env = VaultEnvelope::load_from_dir(&path).unwrap().unwrap();
                let recovered_key =
                    unwrap_master_key_with_password(&saved_env, "Synthetic-fixture-password")
                        .unwrap();
                let saved = open_encrypted_database(
                    &path.join("archive.db"),
                    recovered_key.as_bytes(),
                    false,
                )?;
                assert_eq!(read_stored_page(&saved, &uuid)?.len(), 2);
                assert!(check_encrypted_integrity(&saved)?);
                let bytes = fs::read(path.join("archive.db"))?;
                assert!(!bytes
                    .windows(b"PRIVATE-PAGE-RETIRE-SENTINEL".len())
                    .any(|w| w == b"PRIVATE-PAGE-RETIRE-SENTINEL"));
                // Restoring an old source directory must permit another fully
                // verified retirement, rather than leaving an enabled UI that
                // always refuses because the previous receipt is complete.
                let restored_files = read_stored_page(&saved, &uuid)?;
                drop(saved);
                for file in restored_files {
                    let target = root.join(relative(&uuid)).join(&file.name);
                    fs::create_dir_all(target.parent().unwrap())?;
                    fs::write(target, &file.bytes)?;
                }
                assert!(inspect(conn, &root)?[0].can_retire);
                let again = retire(
                    conn,
                    &root,
                    &key,
                    &env,
                    (&uuid, "Synthetic-fixture-password", CONFIRM),
                    || Ok(()),
                )?;
                assert_ne!(root.join(again.recovery_relative_path), path);
                assert!(!root.join(relative(&uuid)).exists());
                Ok(())
            })
            .unwrap();
        vault.lock().unwrap();
        drop(vault);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn refusal_source_change_cancellation_and_interrupted_removal_are_retryable() {
        let (root, vault, uuid) = fixture();
        let key = vault.master_key().unwrap();
        let env = vault.envelope().unwrap();
        vault
            .with_conn(|conn| {
                assert!(retire(
                    conn,
                    &root,
                    &key,
                    &env,
                    (&uuid, "wrong", CONFIRM),
                    || Ok(())
                )
                .is_err());
                assert!(retire(
                    conn,
                    &root,
                    &key,
                    &env,
                    (&uuid, "Synthetic-fixture-password", ""),
                    || Ok(())
                )
                .is_err());
                assert!(retire(
                    conn,
                    &root,
                    &key,
                    &env,
                    (&uuid, "Synthetic-fixture-password", CONFIRM),
                    || Err(invalid("cancelled"))
                )
                .is_err());
                assert!(root.join(relative(&uuid)).exists());
                assert!(retire_observed(
                    conn,
                    &root,
                    &key,
                    &env,
                    (&uuid, "Synthetic-fixture-password", CONFIRM),
                    || Ok(()),
                    || {
                        fs::remove_file(root.join(relative(&uuid)).join("content.md"))?;
                        Err(invalid("synthetic interruption after one source unlink"))
                    }
                )
                .is_err());
                assert!(root
                    .join(relative(&uuid))
                    .join("assets/unknown.bin")
                    .exists());
                let page = inspect(conn, &root)?.remove(0);
                assert!(page.can_retire && page.resuming);
                assert_eq!(page.file_count, 1);
                // A changed surviving file is never removed, even with a valid receipt.
                fs::write(
                    root.join(relative(&uuid)).join("assets/unknown.bin"),
                    b"changed",
                )?;
                assert!(retire(
                    conn,
                    &root,
                    &key,
                    &env,
                    (&uuid, "Synthetic-fixture-password", CONFIRM),
                    || Ok(())
                )
                .is_err());
                assert!(!inspect(conn, &root)?.remove(0).can_retire);
                fs::write(
                    root.join(relative(&uuid)).join("assets/unknown.bin"),
                    [0, 1, 255],
                )?;
                retire(
                    conn,
                    &root,
                    &key,
                    &env,
                    (&uuid, "Synthetic-fixture-password", CONFIRM),
                    || Ok(()),
                )?;
                assert!(!root.join(relative(&uuid)).exists());
                Ok(())
            })
            .unwrap();
        vault.lock().unwrap();
        drop(vault);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn changed_source_after_pair_validation_is_not_removed() {
        let (root, vault, uuid) = fixture();
        let key = vault.master_key().unwrap();
        let env = vault.envelope().unwrap();
        vault
            .with_conn(|conn| {
                assert!(retire_observed(
                    conn,
                    &root,
                    &key,
                    &env,
                    (&uuid, "Synthetic-fixture-password", CONFIRM),
                    || Ok(()),
                    || {
                        fs::write(
                            root.join(relative(&uuid)).join("new-file"),
                            b"keep new bytes",
                        )?;
                        Ok(())
                    }
                )
                .is_err());
                assert_eq!(
                    fs::read(root.join(relative(&uuid)).join("new-file"))?,
                    b"keep new bytes"
                );
                assert_eq!(
                    fs::read(root.join(relative(&uuid)).join("content.md"))?,
                    b"PRIVATE-PAGE-RETIRE-SENTINEL"
                );
                Ok(())
            })
            .unwrap();
        vault.lock().unwrap();
        drop(vault);
        fs::remove_dir_all(root).unwrap();
    }
}
