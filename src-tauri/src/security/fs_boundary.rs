//! Narrow filesystem helpers used by security-sensitive import and cleanup code.
//!
//! These helpers deliberately reject links and Windows reparse points instead of
//! trying to resolve them.  This keeps callers inside an explicitly selected
//! directory tree and makes an unexpected filesystem object fail closed.

use std::fs::{self, Metadata, OpenOptions};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use crate::error::{AppError, AppResult};

#[cfg(windows)]
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};

#[cfg(windows)]
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
#[cfg(windows)]
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
#[cfg(windows)]
const FILE_SHARE_READ: u32 = 0x0000_0001;

fn boundary_error(message: impl Into<String>) -> AppError {
    AppError::Security(format!("文件边界校验失败: {}", message.into()))
}

/// Return metadata for an ordinary filesystem object.
///
/// `symlink_metadata` is intentional: a link is rejected before any target is
/// inspected.  On Windows, reparse points are rejected as well because not all
/// reparse points are represented as `file_type().is_symlink()`.
pub fn plain_metadata(path: &Path) -> AppResult<Option<Metadata>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() {
        return Err(boundary_error(format!("拒绝符号链接: {}", path.display())));
    }
    #[cfg(windows)]
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(boundary_error(format!(
            "拒绝 Windows 重解析点: {}",
            path.display()
        )));
    }
    Ok(Some(metadata))
}

fn canonical_root(root: &Path) -> AppResult<PathBuf> {
    let metadata = plain_metadata(root)?.ok_or_else(|| boundary_error("根目录不存在"))?;
    if !metadata.is_dir() {
        return Err(boundary_error("根路径不是目录"));
    }
    let canonical = fs::canonicalize(root)?;
    if canonical.parent().is_none()
        || canonical
            .parent()
            .is_some_and(|parent| parent == canonical.as_path())
    {
        return Err(boundary_error("拒绝将文件系统根目录作为边界"));
    }
    Ok(canonical)
}

fn validated_components(relative: &Path) -> AppResult<Vec<Component<'_>>> {
    let components: Vec<_> = relative.components().collect();
    if components.is_empty()
        || components
            .iter()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(boundary_error("相对路径必须非空且只包含普通路径组件"));
    }
    Ok(components)
}

/// Resolve a path while checking every existing component against `root`.
/// Missing final or intermediate components return `Ok(None)`.
pub fn resolve_scoped_path(root: &Path, relative: &Path) -> AppResult<Option<PathBuf>> {
    let canonical_root = canonical_root(root)?;
    let components = validated_components(relative)?;
    let mut current = canonical_root.clone();

    for (index, component) in components.iter().enumerate() {
        current.push(component.as_os_str());
        let Some(metadata) = plain_metadata(&current)? else {
            return Ok(None);
        };
        if index + 1 < components.len() && !metadata.is_dir() {
            return Err(boundary_error(format!(
                "路径组件不是目录: {}",
                current.display()
            )));
        }
    }

    let canonical = fs::canonicalize(&current)?;
    if !canonical.starts_with(&canonical_root) || canonical == canonical_root {
        return Err(boundary_error(format!(
            "解析路径越过根目录边界: {}",
            relative.display()
        )));
    }
    Ok(Some(canonical))
}

/// Read an ordinary file below `root`, with a hard byte limit.
pub fn read_scoped_file(root: &Path, relative: &Path, limit: u64) -> AppResult<Option<Vec<u8>>> {
    let Some(path) = resolve_scoped_path(root, relative)? else {
        return Ok(None);
    };
    let metadata = plain_metadata(&path)?.ok_or_else(|| boundary_error("文件在读取前消失"))?;
    if !metadata.is_file() {
        return Err(boundary_error(format!(
            "目标不是普通文件: {}",
            path.display()
        )));
    }
    if metadata.len() > limit {
        return Err(boundary_error(format!(
            "文件超过读取上限 {} 字节: {}",
            limit,
            path.display()
        )));
    }

    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    options
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    let file = options.open(&path)?;
    let opened_metadata = file.metadata()?;
    if !opened_metadata.is_file() || opened_metadata.file_type().is_symlink() {
        return Err(boundary_error("打开的对象不是普通文件"));
    }
    #[cfg(windows)]
    if opened_metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(boundary_error("打开的对象是 Windows 重解析点"));
    }
    if opened_metadata.len() > limit {
        return Err(boundary_error("打开的文件超过读取上限"));
    }
    let capacity = usize::try_from(
        opened_metadata
            .len()
            .saturating_add(1)
            .min(limit.saturating_add(1)),
    )
    .unwrap_or(usize::MAX);
    let mut contents = Vec::with_capacity(capacity.min(8 * 1024 * 1024));
    file.take(limit.saturating_add(1))
        .read_to_end(&mut contents)?;
    if contents.len() as u64 > limit {
        return Err(boundary_error(format!(
            "文件超过读取上限 {} 字节: {}",
            limit,
            path.display()
        )));
    }
    Ok(Some(contents))
}

/// Enumerate every ordinary file below a scoped directory.
///
/// The returned paths are relative to `root`.  Any unexpected object, read
/// failure, link, or limit violation aborts the whole operation; callers never
/// receive a silently incomplete file set.
pub fn collect_scoped_files(
    root: &Path,
    relative: &Path,
    max_files: usize,
    max_depth: usize,
) -> AppResult<Vec<PathBuf>> {
    let canonical_root = canonical_root(root)?;
    let Some(target) = resolve_scoped_path(&canonical_root, relative)? else {
        return Ok(Vec::new());
    };
    let target_metadata = plain_metadata(&target)?.ok_or_else(|| boundary_error("目标消失"))?;
    if target_metadata.is_file() {
        if max_files == 0 {
            return Err(boundary_error("普通文件数量超过上限"));
        }
        return Ok(vec![target
            .strip_prefix(&canonical_root)
            .map_err(|_| boundary_error("返回路径越过根目录边界"))?
            .to_path_buf()]);
    }
    if !target_metadata.is_dir() {
        return Err(boundary_error(format!(
            "目标不是普通文件或目录: {}",
            target.display()
        )));
    }

    let mut files = Vec::new();
    let mut pending = vec![(target, 0usize)];
    let max_entries = max_files
        .saturating_mul(max_depth.saturating_add(1))
        .saturating_add(1);
    let mut entries_seen = 0usize;
    while let Some((directory, depth)) = pending.pop() {
        let entries = fs::read_dir(&directory)?;
        for entry in entries {
            entries_seen = entries_seen.saturating_add(1);
            if entries_seen > max_entries {
                return Err(boundary_error("目录项目总数超过上限"));
            }
            let entry = entry?;
            let path = entry.path();
            let metadata = plain_metadata(&path)?
                .ok_or_else(|| boundary_error(format!("目录项在检查前消失: {}", path.display())))?;
            if metadata.is_file() {
                if files.len() >= max_files {
                    return Err(boundary_error("普通文件数量超过上限"));
                }
                files.push(
                    path.strip_prefix(&canonical_root)
                        .map_err(|_| boundary_error("返回路径越过根目录边界"))?
                        .to_path_buf(),
                );
            } else if metadata.is_dir() {
                if depth >= max_depth {
                    return Err(boundary_error("目录深度超过上限"));
                }
                pending.push((path, depth + 1));
            } else {
                return Err(boundary_error(format!(
                    "拒绝未知文件类型: {}",
                    path.display()
                )));
            }
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "browsory_fs_boundary_{}_{}",
            label,
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn resolves_reads_and_collects_only_scoped_files() {
        let root = test_root("normal");
        fs::create_dir_all(root.join("nested").join("deep")).unwrap();
        fs::write(root.join("nested").join("one.txt"), b"one").unwrap();
        fs::write(root.join("nested").join("deep").join("two.txt"), b"two").unwrap();
        fs::write(root.join("empty.txt"), b"").unwrap();
        fs::write(root.join("two.txt"), b"two").unwrap();

        assert!(resolve_scoped_path(&root, Path::new("nested/one.txt"))
            .unwrap()
            .is_some());
        assert_eq!(
            read_scoped_file(&root, Path::new("nested/one.txt"), 3).unwrap(),
            Some(b"one".to_vec())
        );
        assert_eq!(
            read_scoped_file(&root, Path::new("empty.txt"), 0).unwrap(),
            Some(Vec::new())
        );
        assert!(read_scoped_file(&root, Path::new("nested/one.txt"), 2).is_err());
        let mut files = collect_scoped_files(&root, Path::new("nested"), 4, 2).unwrap();
        files.sort();
        assert_eq!(
            files,
            vec![
                PathBuf::from("nested/deep/two.txt"),
                PathBuf::from("nested/one.txt")
            ]
        );
        assert!(collect_scoped_files(&root, Path::new("nested"), 0, 2).is_err());
        assert!(collect_scoped_files(&root, Path::new("nested"), 4, 0).is_err());
        fs::create_dir(root.join("empty-tree")).unwrap();
        for index in 0..4 {
            fs::create_dir(root.join("empty-tree").join(index.to_string())).unwrap();
        }
        assert!(collect_scoped_files(&root, Path::new("empty-tree"), 1, 1).is_err());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_invalid_paths_roots_and_links() {
        let root = test_root("reject");
        fs::write(root.join("file"), b"x").unwrap();
        assert!(resolve_scoped_path(&root, Path::new("../file")).is_err());
        assert!(resolve_scoped_path(&root, Path::new("/file")).is_err());
        assert!(resolve_scoped_path(&root, Path::new("")).is_err());
        assert!(resolve_scoped_path(Path::new("/"), Path::new("file")).is_err());

        let outside = test_root("outside");
        fs::write(outside.join("sentinel"), b"keep").unwrap();
        let link = root.join("link");
        #[cfg(unix)]
        let link_result = std::os::unix::fs::symlink(&outside, &link);
        #[cfg(windows)]
        let link_result = std::os::windows::fs::symlink_dir(&outside, &link);
        if link_result.is_ok() {
            assert!(plain_metadata(&link).is_err());
            assert!(resolve_scoped_path(&root, Path::new("link/sentinel")).is_err());
            assert!(collect_scoped_files(&root, Path::new("link"), 10, 2).is_err());
            #[cfg(windows)]
            fs::remove_dir(&link).unwrap();
            #[cfg(unix)]
            fs::remove_file(&link).unwrap();
        }
        assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"keep");
        fs::remove_dir_all(outside).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn actual_windows_junction_is_rejected_and_cleanup_preserves_external_files() {
        use std::os::windows::process::CommandExt;
        let root = test_root("junction");
        let outside = test_root("junction_external");
        fs::write(outside.join("sentinel"), b"keep external bytes").unwrap();
        let uuid = uuid::Uuid::new_v4().to_string();
        let relative = Path::new("temp").join(uuid);
        let candidate = root.join(&relative);
        fs::create_dir_all(&candidate).unwrap();
        let link = candidate.join("outside");
        let result = std::process::Command::new("powershell.exe")
            .args(["-NoProfile","-NonInteractive","-Command", "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:BROWSORY_JUNCTION_LINK -Target $env:BROWSORY_JUNCTION_TARGET | Out-Null"])
            .env("BROWSORY_JUNCTION_LINK", &link).env("BROWSORY_JUNCTION_TARGET", &outside)
            .creation_flags(0x08000000).output().unwrap();
        assert!(
            result.status.success(),
            "junction creation failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(plain_metadata(&link).is_err());
        assert!(resolve_scoped_path(&root, &relative.join("outside/sentinel")).is_err());
        assert!(collect_scoped_files(&root, &relative, 128, 8).is_err());
        assert_eq!(
            crate::history::snapshot::cleanup_orphan_snapshots(&root, &root.join("temp")),
            (0, 0)
        );
        assert!(candidate.exists());
        assert_eq!(
            crate::commands::storage::dir_size_and_count(&candidate),
            (0, 0)
        );
        assert_eq!(
            fs::read(outside.join("sentinel")).unwrap(),
            b"keep external bytes"
        );
        // Remove only the junction itself, then verify the external target is intact.
        assert!(
            fs::symlink_metadata(&link).unwrap().file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT
                != 0
        );
        fs::remove_dir(&link).unwrap();
        assert_eq!(
            fs::read(outside.join("sentinel")).unwrap(),
            b"keep external bytes"
        );
        let temporary = fs::canonicalize(std::env::temp_dir()).unwrap();
        assert!(fs::canonicalize(&root).unwrap().starts_with(&temporary));
        assert!(fs::canonicalize(&outside).unwrap().starts_with(&temporary));
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }
}
