//! OS-held lock, not a PID file: it is released when the process terminates.
use std::fs::{self, File, OpenOptions};
use std::path::Path;

use crate::error::{AppError, AppResult};

pub enum LockAcquireError {
    AlreadyHeld(AppError),
    Other(AppError),
}

fn is_lock_held_io_error(err: &std::io::Error) -> bool {
    if err.kind() == std::io::ErrorKind::WouldBlock {
        return true;
    }
    #[cfg(windows)]
    if matches!(err.raw_os_error(), Some(32 | 33)) {
        // ERROR_SHARING_VIOLATION (32) or ERROR_LOCK_VIOLATION (33)
        return true;
    }
    false
}

pub struct DataDirectoryLock {
    _file: File,
}

impl DataDirectoryLock {
    pub fn acquire(directory: &Path) -> AppResult<Self> {
        Self::acquire_classified(directory).map_err(|err| match err {
            LockAcquireError::AlreadyHeld(app_err) | LockAcquireError::Other(app_err) => app_err,
        })
    }

    pub fn acquire_classified(directory: &Path) -> Result<Self, LockAcquireError> {
        fs::create_dir_all(directory).map_err(|e| LockAcquireError::Other(AppError::Io(e)))?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // Also prohibit deleting/replacing the lock file while held.
            options.share_mode(0);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options
            .open(directory.join("vault.instance.lock"))
            .map_err(|e| {
                if is_lock_held_io_error(&e) {
                    LockAcquireError::AlreadyHeld(AppError::Security(format!(
                        "无法独占数据目录：已有 Browsory 实例正在运行 ({e})"
                    )))
                } else {
                    LockAcquireError::Other(AppError::Security(format!(
                        "无法独占数据目录：目录不可写或 I/O 失败 ({e})"
                    )))
                }
            })?;
        fs2::FileExt::try_lock_exclusive(&file).map_err(|e| {
            if is_lock_held_io_error(&e) {
                LockAcquireError::AlreadyHeld(AppError::Security(format!(
                    "数据目录已被其他实例占用，拒绝启动 ({e})"
                )))
            } else {
                LockAcquireError::Other(AppError::Security(format!("数据目录排他锁失败 ({e})")))
            }
        })?;
        Ok(Self { _file: file })
    }

    pub fn acquire_with_retry(
        directory: &Path,
        max_attempts: usize,
        delay: std::time::Duration,
    ) -> AppResult<Self> {
        Self::acquire_with_retry_classified(directory, max_attempts, delay).map_err(|err| match err
        {
            LockAcquireError::AlreadyHeld(app_err) | LockAcquireError::Other(app_err) => app_err,
        })
    }

    pub fn acquire_with_retry_classified(
        directory: &Path,
        max_attempts: usize,
        delay: std::time::Duration,
    ) -> Result<Self, LockAcquireError> {
        let attempts = max_attempts.max(1);
        let mut last_err = None;
        for attempt in 0..attempts {
            match Self::acquire_classified(directory) {
                Ok(lock) => return Ok(lock),
                Err(LockAcquireError::Other(err)) => return Err(LockAcquireError::Other(err)),
                Err(LockAcquireError::AlreadyHeld(err)) => {
                    last_err = Some(err);
                    if attempt + 1 < attempts {
                        std::thread::sleep(delay);
                    }
                }
            }
        }
        Err(LockAcquireError::AlreadyHeld(last_err.unwrap_or_else(
            || AppError::Security("数据目录锁获取失败".into()),
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subprocess_probe() {
        let Some(root) = std::env::var_os("BROWSORY_LOCK_TEST_DIRECTORY") else {
            return;
        };
        let path = std::path::PathBuf::from(root);
        if std::env::var_os("BROWSORY_LOCK_TEST_HOLD").is_some() {
            let _owner = DataDirectoryLock::acquire(&path).unwrap();
            fs::write(path.join("child-ready"), b"ready").unwrap();
            std::thread::sleep(std::time::Duration::from_secs(30));
        } else {
            assert!(DataDirectoryLock::acquire(&path).is_err());
        }
    }

    #[test]
    fn another_process_is_denied_and_forced_exit_releases_ownership() {
        use std::process::Command;
        use std::time::{Duration, Instant};
        let root =
            std::env::temp_dir().join(format!("browsory_process_lock_{}", uuid::Uuid::new_v4()));
        let first = DataDirectoryLock::acquire(&root).unwrap();
        let make_command = || {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "security::directory_lock::tests::subprocess_probe",
                    "--test-threads=1",
                ])
                .env("BROWSORY_LOCK_TEST_DIRECTORY", &root);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000);
            }
            command
        };
        assert!(make_command().output().unwrap().status.success());
        drop(first);
        let mut child = make_command()
            .env("BROWSORY_LOCK_TEST_HOLD", "1")
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !root.join("child-ready").exists() {
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("lock child failed to start");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(DataDirectoryLock::acquire(&root).is_err());
        child.kill().unwrap();
        child.wait().unwrap();
        let released = DataDirectoryLock::acquire(&root).unwrap();
        drop(released);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn exclusive_ownership_is_released_with_handle_not_stale_file() {
        let root =
            std::env::temp_dir().join(format!("browsory_directory_lock_{}", uuid::Uuid::new_v4()));
        let first = DataDirectoryLock::acquire(&root).unwrap();
        assert!(DataDirectoryLock::acquire(&root).is_err());
        assert!(root.join("vault.instance.lock").exists());
        drop(first);
        let next = DataDirectoryLock::acquire(&root).unwrap();
        drop(next);
        fs::remove_dir_all(root).unwrap();
    }
}
