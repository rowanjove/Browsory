//! Native Windows suspend/resume notifications for vault locking.
//!
//! The operating-system callback is deliberately small: it revokes the
//! current vault session immediately and unparks a pre-started worker for
//! frontend notifications plus the potentially blocking drain/close. It never
//! initiates sleep or hibernation itself.

use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};

static LOCK_ON_SLEEP: AtomicBool = AtomicBool::new(true);
static REGISTERED: AtomicBool = AtomicBool::new(false);

const LOCK_EVENT: &str = "browsory://lock";
const POWER_EVENT: &str = "browsory://power";

/// Non-sensitive state visible to the current application session.
#[derive(Debug, Clone, Serialize)]
pub struct PowerEvent {
    pub kind: &'static str,
    pub phase: &'static str,
    pub locked: bool,
}

/// Updates the cached policy used by the native callback. The persisted
/// setting is owned by the security command; this cache avoids database work
/// on the Windows notification thread.
pub fn set_lock_on_sleep(enabled: bool) {
    LOCK_ON_SLEEP.store(enabled, Ordering::Release);
}

fn lock_on_sleep() -> bool {
    LOCK_ON_SLEEP.load(Ordering::Acquire)
}

pub fn supported() -> bool {
    REGISTERED.load(Ordering::Acquire)
}

#[cfg(windows)]
mod windows_impl {
    use super::{lock_on_sleep, PowerEvent, LOCK_EVENT, POWER_EVENT, REGISTERED};
    use crate::database::DbState;
    use std::ffi::c_void;
    use std::ptr::null_mut;
    use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
    use std::sync::{Arc, OnceLock};
    use std::thread::{self, Thread};
    use tauri::{Emitter, Manager};
    use tracing::{debug, warn};
    use windows_sys::Win32::Foundation::{ERROR_SUCCESS, HANDLE};
    use windows_sys::Win32::System::Power::{
        PowerRegisterSuspendResumeNotification, PowerUnregisterSuspendResumeNotification,
        DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS, HPOWERNOTIFY,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DEVICE_NOTIFY_CALLBACK, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND, PBT_APMSUSPEND,
    };

    const PENDING_SUSPEND: u8 = 1;
    const PENDING_POLICY_IGNORED: u8 = 1 << 1;
    const PENDING_RESUME: u8 = 1 << 2;
    const PENDING_LOCK_RESUME: u8 = 1 << 3;

    /// State shared by the native callback and one pre-started worker. The
    /// callback only touches atomics, revokes the active session, and unparks
    /// the worker; it never enters Tauri state or emits an event.
    struct PowerRuntime {
        app: tauri::AppHandle,
        vault: Arc<crate::security::vault::VaultManager>,
        pending: AtomicU8,
        stop: AtomicBool,
        worker: OnceLock<Thread>,
    }

    // Process-lifetime callback state avoids assuming that unregister waits
    // for every in-flight callback. There is only one subscription per app.
    static CALLBACK_RUNTIME: OnceLock<Arc<PowerRuntime>> = OnceLock::new();

    /// Owns the native registration and callback context until application
    /// shutdown. The worker is intentionally detached: shutdown signals it but
    /// never waits for an in-flight vault drain on the UI/native thread.
    pub struct PowerSubscription {
        registration: HPOWERNOTIFY,
        runtime: Arc<PowerRuntime>,
    }

    impl Drop for PowerSubscription {
        fn drop(&mut self) {
            REGISTERED.store(false, Ordering::Release);
            self.runtime.stop.store(true, Ordering::Release);
            if self.registration != 0 {
                let result = unsafe { PowerUnregisterSuspendResumeNotification(self.registration) };
                if result != ERROR_SUCCESS {
                    warn!(
                        "PowerUnregisterSuspendResumeNotification failed: {}",
                        result
                    );
                    // Callback state remains valid until process exit even
                    // when unregister cannot confirm that callbacks stopped.
                }
            }

            self.runtime.stop.store(true, Ordering::Release);
            if let Some(worker) = self.runtime.worker.get() {
                worker.unpark();
            }
        }
    }

    fn emit(app: &tauri::AppHandle, event: &str, payload: PowerEvent) {
        if let Err(error) = app.emit(event, payload) {
            debug!(event, %error, "power lifecycle event had no active listener");
        }
    }

    fn process_pending(runtime: &PowerRuntime, pending: u8) {
        if pending & (PENDING_SUSPEND | PENDING_LOCK_RESUME) != 0 {
            let kind = if pending & PENDING_SUSPEND != 0 {
                "suspend"
            } else {
                "resume"
            };
            emit(
                &runtime.app,
                LOCK_EVENT,
                PowerEvent {
                    kind,
                    phase: "requested",
                    locked: false,
                },
            );

            let result = runtime.vault.lock();
            emit(
                &runtime.app,
                POWER_EVENT,
                PowerEvent {
                    kind,
                    phase: "completed",
                    locked: result.is_ok(),
                },
            );
            if let Err(error) = result {
                warn!("power-triggered vault lock did not complete: {}", error);
            }
        }

        if pending & PENDING_POLICY_IGNORED != 0 {
            emit(
                &runtime.app,
                POWER_EVENT,
                PowerEvent {
                    kind: "suspend",
                    phase: "ignored",
                    locked: false,
                },
            );
        }

        if pending & PENDING_RESUME != 0 && pending & PENDING_LOCK_RESUME == 0 {
            emit(
                &runtime.app,
                POWER_EVENT,
                PowerEvent {
                    kind: "resume",
                    phase: "observed",
                    locked: false,
                },
            );
        }
    }

    fn worker_loop(runtime: Arc<PowerRuntime>) {
        loop {
            thread::park();
            if runtime.stop.load(Ordering::Acquire) {
                break;
            }
            let pending = runtime.pending.swap(0, Ordering::AcqRel);
            if pending != 0 {
                process_pending(&runtime, pending);
            }
        }
    }

    unsafe extern "system" fn power_callback(
        _context: *const c_void,
        notification_type: u32,
        _setting: *const c_void,
    ) -> u32 {
        let Some(runtime) = CALLBACK_RUNTIME.get() else {
            return 0;
        };
        if runtime.stop.load(Ordering::Acquire) {
            return 0;
        }
        match notification_type {
            PBT_APMSUSPEND => {
                if lock_on_sleep() {
                    // The callback path is deliberately atomic-only. The
                    // actual close/drain and all UI events run on worker_loop.
                    runtime.vault.revoke_active_session();
                    runtime.pending.fetch_or(PENDING_SUSPEND, Ordering::Release);
                } else {
                    runtime
                        .pending
                        .fetch_or(PENDING_POLICY_IGNORED, Ordering::Release);
                }
                if let Some(worker) = runtime.worker.get() {
                    worker.unpark();
                }
            }
            PBT_APMRESUMESUSPEND | PBT_APMRESUMEAUTOMATIC => {
                if lock_on_sleep() {
                    runtime.vault.revoke_active_session();
                    runtime
                        .pending
                        .fetch_or(PENDING_LOCK_RESUME, Ordering::Release);
                }
                runtime.pending.fetch_or(PENDING_RESUME, Ordering::Release);
                if let Some(worker) = runtime.worker.get() {
                    worker.unpark();
                }
            }
            _ => {}
        }

        0
    }

    pub fn install(app: tauri::AppHandle) -> Result<PowerSubscription, String> {
        let vault = app
            .try_state::<DbState>()
            .map(|state| state.vault.clone())
            .ok_or_else(|| "数据库状态尚未安装，无法订阅电源通知".to_string())?;
        let runtime = Arc::new(PowerRuntime {
            app,
            vault,
            pending: AtomicU8::new(0),
            stop: AtomicBool::new(false),
            worker: OnceLock::new(),
        });
        let worker_runtime = runtime.clone();
        let worker = thread::Builder::new()
            .name("browsory-power-lock".to_string())
            .spawn(move || worker_loop(worker_runtime))
            .map_err(|error| format!("无法启动电源锁后台线程: {}", error))?;
        if runtime.worker.set(worker.thread().clone()).is_err() {
            runtime.stop.store(true, Ordering::Release);
            worker.thread().unpark();
            return Err("电源锁后台线程已重复初始化".to_string());
        }
        // Drop the join handle deliberately. Runtime owns the thread through
        // its Arc and Drop signals it without waiting for a long drain.
        drop(worker);

        if CALLBACK_RUNTIME.set(runtime.clone()).is_err() {
            runtime.stop.store(true, Ordering::Release);
            if let Some(worker) = runtime.worker.get() {
                worker.unpark();
            }
            return Err("电源通知订阅已初始化，请重启后重试".into());
        }
        let mut subscription = DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
            Callback: Some(power_callback),
            Context: null_mut(),
        };
        let mut registration: *mut c_void = null_mut();
        let result = unsafe {
            PowerRegisterSuspendResumeNotification(
                DEVICE_NOTIFY_CALLBACK,
                (&mut subscription as *mut DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS).cast::<c_void>()
                    as HANDLE,
                &mut registration,
            )
        };

        if result != ERROR_SUCCESS {
            // SAFETY: registration failed, so ownership remains here.

            runtime.stop.store(true, Ordering::Release);
            if let Some(worker) = runtime.worker.get() {
                worker.unpark();
            }
            return Err(format!(
                "PowerRegisterSuspendResumeNotification failed: {}",
                result
            ));
        }

        REGISTERED.store(true, Ordering::Release);
        Ok(PowerSubscription {
            registration: registration as HPOWERNOTIFY,
            runtime,
        })
    }
}

#[cfg(windows)]
pub use windows_impl::{install, PowerSubscription};

#[cfg(not(windows))]
pub struct PowerSubscription;

#[cfg(not(windows))]
pub fn install(_app: tauri::AppHandle) -> Result<PowerSubscription, String> {
    Err("native suspend/resume notifications are only supported on Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_cache_defaults_to_locking_and_can_be_updated() {
        set_lock_on_sleep(true);
        assert!(lock_on_sleep());
        set_lock_on_sleep(false);
        assert!(!lock_on_sleep());
        set_lock_on_sleep(true);
    }
}
