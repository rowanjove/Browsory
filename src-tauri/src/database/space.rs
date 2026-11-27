//! Conservative disk-space checks for database snapshots and migrations.
//!
//! The returned values are deliberately estimates with headroom, not an exact
//! prediction of SQLite/SQLCipher's allocation pattern.  Callers must still
//! handle a later `ENOSPC` while an operation is in progress.

use std::path::Path;

use crate::error::{AppError, AppResult};

const MIB: u64 = 1024 * 1024;
const RESERVED_HEADROOM_BYTES: u64 = 64 * MIB;

fn overflow_error() -> AppError {
    AppError::Migration("数据库迁移所需空间估算溢出".into())
}

/// Check the free space on the filesystem containing `directory`.
pub fn require_available_space(directory: &Path, required: u64) -> AppResult<()> {
    let available = fs2::available_space(directory)?;
    if available < required {
        return Err(AppError::Migration(format!(
            "数据库迁移前可用空间不足：至少需要 {} 字节，当前仅有 {} 字节",
            required, available
        )));
    }
    Ok(())
}

/// Estimate the space needed to retain a protected snapshot of one file.
///
/// Four times the source size leaves room for a source copy, a transformed
/// copy, verification/temporary SQLite files, and filesystem allocation
/// variance.  The fixed reserve protects small databases from underestimating
/// journals and metadata.  This is intentionally conservative and may reject
/// a migration that would happen to fit exactly.
pub fn protected_snapshot_requirement(source_bytes: u64) -> AppResult<u64> {
    source_bytes
        .checked_mul(4)
        .and_then(|bytes| bytes.checked_add(RESERVED_HEADROOM_BYTES))
        .ok_or_else(overflow_error)
}

/// Estimate the free space needed for plaintext-to-SQLCipher migration.
///
/// `main_bytes` and `wal_bytes` are the observed sizes before any checkpoint.
/// The estimate includes the source plus WAL, encrypted staging, the verified
/// recovery copy, possible checkpoint/upgrade journals and a second fixed
/// reserve.  It is not an exact allocation contract; every write path still
/// propagates its own I/O errors.
pub fn migration_requirement(main_bytes: u64, wal_bytes: u64) -> AppResult<u64> {
    let source_bytes = main_bytes
        .checked_add(wal_bytes)
        .ok_or_else(overflow_error)?;
    protected_snapshot_requirement(source_bytes)?
        .checked_add(source_bytes)
        .and_then(|bytes| bytes.checked_add(RESERVED_HEADROOM_BYTES))
        .ok_or_else(overflow_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_snapshot_has_fixed_headroom_and_checked_overflow() {
        assert_eq!(
            protected_snapshot_requirement(10 * MIB).unwrap(),
            40 * MIB + RESERVED_HEADROOM_BYTES
        );
        assert!(protected_snapshot_requirement(u64::MAX).is_err());
    }

    #[test]
    fn migration_estimate_includes_main_wal_and_extra_headroom() {
        let main = 10 * MIB;
        let wal = 2 * MIB;
        assert_eq!(
            migration_requirement(main, wal).unwrap(),
            5 * (main + wal) + 2 * RESERVED_HEADROOM_BYTES
        );
        assert!(migration_requirement(u64::MAX, 1).is_err());
    }

    #[test]
    fn available_space_rejects_synthetic_insufficient_capacity() {
        let directory = std::env::temp_dir();
        assert!(require_available_space(&directory, 0).is_ok());
        assert!(require_available_space(&directory, u64::MAX).is_err());
    }
}
