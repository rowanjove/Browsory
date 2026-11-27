//! Process-wide mutual exclusion for history sync operations (watcher, manual, startup, tray due).
use std::sync::{Mutex, OnceLock};

use crate::database::models::SyncTriggerType;
use crate::error::{AppError, AppResult};

static COORDINATOR: OnceLock<SyncCoordinator> = OnceLock::new();

pub fn global_sync_coordinator() -> &'static SyncCoordinator {
    COORDINATOR.get_or_init(SyncCoordinator::new)
}

#[derive(Debug, Clone, Copy)]
pub struct ActiveSyncInfo {
    pub trigger: SyncTriggerType,
    pub started_at_ms: i64,
}

pub struct SyncCoordinator {
    active: Mutex<Option<ActiveSyncInfo>>,
}

pub struct SyncLease<'a> {
    coordinator: &'a SyncCoordinator,
}

impl Drop for SyncLease<'_> {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.coordinator.active.lock() {
            *guard = None;
        }
    }
}

impl SyncCoordinator {
    pub fn new() -> Self {
        Self {
            active: Mutex::new(None),
        }
    }

    pub fn try_acquire(&self, trigger: SyncTriggerType) -> Option<SyncLease<'_>> {
        let mut guard = self.active.lock().ok()?;
        if guard.is_some() {
            return None;
        }
        *guard = Some(ActiveSyncInfo {
            trigger,
            started_at_ms: chrono::Utc::now().timestamp_millis(),
        });
        Some(SyncLease { coordinator: self })
    }

    pub fn acquire_or_reject(&self, trigger: SyncTriggerType) -> AppResult<SyncLease<'_>> {
        let mut guard = self
            .active
            .lock()
            .map_err(|_| AppError::System("同步互斥锁不可用".into()))?;
        if let Some(current) = *guard {
            return Err(AppError::Sync(format!(
                "已有同步任务正在执行 ({})，请稍候",
                current.trigger.as_str()
            )));
        }
        *guard = Some(ActiveSyncInfo {
            trigger,
            started_at_ms: chrono::Utc::now().timestamp_millis(),
        });
        Ok(SyncLease { coordinator: self })
    }

    pub fn is_running(&self) -> bool {
        self.active
            .lock()
            .map(|guard| guard.is_some())
            .unwrap_or(false)
    }

    pub fn active_info(&self) -> Option<ActiveSyncInfo> {
        self.active.lock().ok().and_then(|guard| *guard)
    }
}

impl Default for SyncCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinator_enforces_mutual_exclusion_and_releases_on_drop() {
        let coord = SyncCoordinator::new();
        assert!(!coord.is_running());

        let lease = coord.try_acquire(SyncTriggerType::Scheduled).unwrap();
        assert!(coord.is_running());
        assert!(coord.try_acquire(SyncTriggerType::Watcher).is_none());
        assert!(coord.acquire_or_reject(SyncTriggerType::Manual).is_err());

        drop(lease);
        assert!(!coord.is_running());
        let lease2 = coord.acquire_or_reject(SyncTriggerType::Manual).unwrap();
        assert!(coord.is_running());
        drop(lease2);
        assert!(!coord.is_running());
    }
}
