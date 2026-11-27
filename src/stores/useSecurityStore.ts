import { create } from 'zustand';
import { tauriApi } from '../services/tauri';
import { describeSyncResults } from '../services/syncFeedback';
import { SecurityState, SyncResult, VaultStatus, VaultStatusInfo } from '../types';
import { useHistoryStore } from './useHistoryStore';
import { closeSensitiveSession, openSensitiveSession } from '../services/vaultSession';

const isScreenshotPreview =
  import.meta.env.DEV && typeof window !== 'undefined' && new URLSearchParams(window.location.search).has('preview');

// Every lock transition invalidates in-flight sensitive requests. The value is
// intentionally module-local: it is a frontend freshness guard, not an auth token.
let securityEpoch = 0;
let activeLockEpoch: number | null = null;
let transitionTail: Promise<void> = Promise.resolve();

// FIFO dispatch is separate from response freshness. A lock requested while an
// unlock is running must reach Rust after that unlock, rather than merely hide
// its response while leaving the backend reopened.
function vaultTransition<T>(request: () => Promise<T>): Promise<T> {
  const next = transitionTail.then(request, request);
  transitionTail = next.then(() => undefined, () => undefined);
  return next;
}

interface SecurityStore {
  isInitialized: boolean;
  isLocked: boolean; // True if vault is locked OR UI PIN screen lock is active
  isUiLocked: boolean;
  vaultStatus: VaultStatus;
  hasEnvelope: boolean;
  hasDpapiWrap: boolean;
  passiveResumeError: string | null;
  pinRequiredOnResume: boolean;
  hasPlaintextDb: boolean;
  isPortable: boolean;
  vaultLockError: string | null;
  isVaultLocking: boolean;
  isVaultRestoring: boolean;
  recoveryKeyPending: boolean;
  pendingRecoveryKey: string | null;

  pinEnabled: boolean;
  pinMigrationNeeded: boolean;
  autoLockMinutes: number;
  lockOnMinimize: boolean;
  lockOnSleep: boolean;
  nativeSleepLockSupported: boolean;
  isLockedOut: boolean;
  lockoutRemainingSecs: number;
  hasSyncedAfterStart: boolean;
  isSyncing: boolean;
  lastSyncTime: number | null;
  lastSyncCount: number;
  lastSyncError: string | null;
  lastSyncResults: SyncResult[];
  recordSyncResults: (results: SyncResult[]) => void;
  refreshSyncStatus: () => Promise<void>;

  checkSecurityState: (options?: { relockIfPinEnabled?: boolean }) => Promise<void>;
  createVault: (password: string) => Promise<string>;
  confirmVaultRecoveryKey: (key: string) => Promise<void>;
  regeneratePassiveVaultRecoveryKey: () => Promise<string>;
  unlockVault: (password: string) => Promise<boolean>;
  unlockVaultWithRecoveryKey: (recoveryKey: string) => Promise<boolean>;
  migrateLegacyDatabase: (password: string) => Promise<string>;
  lockVault: () => Promise<void>;
  restoreVaultBackup: (source: number | string, password?: string, recoveryKey?: string) => Promise<string>;
  changeVaultPassword: (oldPassword: string, newPassword: string) => Promise<void>;

  unlock: (pin: string) => Promise<boolean>;
  unlockWithRecoveryKey: (recoveryKey: string) => Promise<boolean>;
  resetPinWithRecoveryKey: (recoveryKey: string, newPin: string) => Promise<void>;
  lock: () => void;
  setOrChangePin: (oldPin: string | null, newPin: string) => Promise<string | null>;
  disablePin: (currentPin: string) => Promise<void>;
  updateOptions: (autoLockMinutes: number, lockOnMinimize: boolean, lockOnSleep: boolean) => Promise<void>;
  triggerFirstUnlockSyncIfNeeded: () => Promise<void>;
  triggerSyncAll: (options?: { silentIfBusy?: boolean }) => Promise<number>;
}

export const useSecurityStore = create<SecurityStore>((set, get) => ({
  isInitialized: isScreenshotPreview,
  isLocked: !isScreenshotPreview, // Fail closed outside the local preview.
  isUiLocked: false,
  vaultStatus: 'locked',
  hasEnvelope: false,
  hasDpapiWrap: false,
  passiveResumeError: null,
  pinRequiredOnResume: false,
  hasPlaintextDb: false,
  isPortable: false,
  vaultLockError: null,
  isVaultLocking: false,
  isVaultRestoring: false,
  recoveryKeyPending: false,
  pendingRecoveryKey: null,

  pinEnabled: false,
  pinMigrationNeeded: false,
  autoLockMinutes: 15,
  lockOnMinimize: false,
  lockOnSleep: true,
  nativeSleepLockSupported: false,
  isLockedOut: false,
  lockoutRemainingSecs: 0,
  hasSyncedAfterStart: false,
  isSyncing: false,
  lastSyncTime: null,
  lastSyncCount: 0,
  lastSyncError: null,
  lastSyncResults: [],

  checkSecurityState: async (options) => {
    if (isScreenshotPreview) return;
    const requestEpoch = securityEpoch;
    const relockIfPinEnabled = options?.relockIfPinEnabled !== false;
    try {
      let vInfo: VaultStatusInfo = await tauriApi.getVaultStatus();
      if (requestEpoch !== securityEpoch) return;
      let hasDpapiWrap = Boolean(vInfo.has_dpapi_wrap);
      let passiveResumeError: string | null = null;

      if (vInfo.status === 'locked' && hasDpapiWrap) {
        const resumed = await vaultTransition(() => tauriApi.resumePassiveVault()).catch((error) => {
          passiveResumeError = error instanceof Error ? error.message : String(error);
          return null;
        });
        if (requestEpoch !== securityEpoch) return;
        if (resumed?.unlocked) {
          vInfo = await tauriApi.getVaultStatus();
          if (requestEpoch !== securityEpoch) return;
          hasDpapiWrap = Boolean(vInfo.has_dpapi_wrap);
        } else if (resumed && !resumed.unlocked && resumed.pin_required) {
          set({
            isInitialized: true,
            vaultStatus: vInfo.status,
            hasEnvelope: vInfo.has_envelope,
            hasDpapiWrap,
            passiveResumeError: null,
            pinRequiredOnResume: true,
            hasPlaintextDb: vInfo.has_plaintext_db,
            isPortable: vInfo.is_portable,
            recoveryKeyPending: false,
            pinEnabled: true,
            isLocked: true,
            isUiLocked: true,
            isLockedOut: resumed.is_locked_out,
            lockoutRemainingSecs: resumed.lockout_remaining_secs,
            vaultLockError: null,
          });
          return;
        }
      }

      const vaultLocked = vInfo.status !== 'unlocked';
      const existingLockError = get().vaultLockError;
      const lockInFlight = activeLockEpoch !== null;
      const preserveFailedLockOverlay = Boolean(existingLockError && get().isLocked && !vaultLocked);

      let pinEnabled = false;
      let pinMigrationNeeded = false;
      let autoLockMinutes = 15;
      let lockOnMinimize = false;
      let lockOnSleep = true;
      let nativeSleepLockSupported = false;
      let isLockedOut = false;
      let lockoutRemainingSecs = 0;

      if (!vaultLocked) {
        try {
          const secState: SecurityState = await tauriApi.getSecurityState();
          if (requestEpoch !== securityEpoch) return;
          pinEnabled = secState.pin_enabled;
          pinMigrationNeeded = Boolean(secState.pin_migration_needed);
          autoLockMinutes = secState.auto_lock_minutes;
          lockOnMinimize = secState.lock_on_minimize;
          lockOnSleep = secState.lock_on_sleep;
          nativeSleepLockSupported = secState.native_sleep_lock_supported === true;
          isLockedOut = secState.is_locked_out;
          lockoutRemainingSecs = secState.lockout_remaining_secs;
        } catch (e) {
          console.warn('Could not read PIN state:', e);
        }
      }

      if (requestEpoch !== securityEpoch) return;
      const recoveryPending = vInfo.recovery_key_pending;
      const shouldLock = vaultLocked || recoveryPending || (pinEnabled && (relockIfPinEnabled || get().isUiLocked));
      const keepProtected = lockInFlight || preserveFailedLockOverlay;

      set({
        isInitialized: true,
        vaultStatus: vInfo.status,
        hasEnvelope: vInfo.has_envelope,
        hasDpapiWrap,
        passiveResumeError,
        pinRequiredOnResume: false,
        hasPlaintextDb: vInfo.has_plaintext_db,
        isPortable: vInfo.is_portable,
        recoveryKeyPending: recoveryPending,
        pinEnabled,
        pinMigrationNeeded,
        autoLockMinutes,
        lockOnMinimize,
        lockOnSleep,
        nativeSleepLockSupported,
        isLockedOut,
        lockoutRemainingSecs,
        isLocked: shouldLock || keepProtected,
        isUiLocked: keepProtected ? false : pinEnabled && shouldLock && !vaultLocked && !recoveryPending,
        vaultLockError: preserveFailedLockOverlay ? existingLockError : null,
      });

      if (!shouldLock && !keepProtected && !get().hasSyncedAfterStart) {
        get().triggerFirstUnlockSyncIfNeeded();
      }
      if (
        !vaultLocked && recoveryPending && hasDpapiWrap &&
        !get().pendingRecoveryKey
      ) {
        try {
          const key = await vaultTransition(() => tauriApi.regeneratePassiveVaultRecoveryKey());
          if (requestEpoch !== securityEpoch) return;
          set({ pendingRecoveryKey: key });
        } catch (error) {
          if (requestEpoch !== securityEpoch) return;
          set({ passiveResumeError: error instanceof Error ? error.message : String(error) });
        }
      }
    } catch (e) {
      console.error('Failed to get security/vault state:', e);
      if (requestEpoch !== securityEpoch) return;
      set({ isInitialized: true, isLocked: true });
    }
  },

  createVault: async (password: string) => {
    const requestEpoch = securityEpoch;
    const recoveryKey = await vaultTransition(() => tauriApi.createVault(password));
    if (requestEpoch !== securityEpoch) throw new Error('保险库会话已变化，请重新解锁完成恢复密钥确认');
    set({
      vaultStatus: 'unlocked',
      // Keep the setup screen mounted until the recovery key is acknowledged.
      isLocked: true,
      isUiLocked: false,
      hasEnvelope: true,
      hasPlaintextDb: false,
      recoveryKeyPending: true,
      pendingRecoveryKey: recoveryKey,
    });
    return recoveryKey;
  },

  confirmVaultRecoveryKey: async (key: string) => {
    const requestEpoch = securityEpoch;
    await tauriApi.confirmVaultRecoveryKey(key);
    if (requestEpoch !== securityEpoch) return;
    set({ recoveryKeyPending: false, pendingRecoveryKey: null });
    await get().checkSecurityState();
    if (requestEpoch !== securityEpoch) return;
  },

  regeneratePassiveVaultRecoveryKey: async () => {
    const requestEpoch = securityEpoch;
    const key = await vaultTransition(() => tauriApi.regeneratePassiveVaultRecoveryKey());
    if (requestEpoch !== securityEpoch) throw new Error('保险库会话已变化，请重新确认恢复密钥');
    set({ pendingRecoveryKey: key, passiveResumeError: null });
    return key;
  },

  unlockVault: async (password: string) => {
    const requestEpoch = securityEpoch;
    if (activeLockEpoch !== null) return false;
    const ok = await vaultTransition(() => tauriApi.unlockVault(password));
    if (ok && requestEpoch === securityEpoch) {
      await get().checkSecurityState({ relockIfPinEnabled: false });
      if (requestEpoch !== securityEpoch) return false;
      if (get().recoveryKeyPending) {
        const key = await tauriApi.regenerateVaultRecoveryKey(password);
        if (requestEpoch !== securityEpoch) return false;
        set({ pendingRecoveryKey: key });
      } else {
        get().triggerFirstUnlockSyncIfNeeded();
      }
      return true;
    }
    return false;
  },

  unlockVaultWithRecoveryKey: async (recoveryKey: string) => {
    const requestEpoch = securityEpoch;
    if (activeLockEpoch !== null) return false;
    const ok = await vaultTransition(() => tauriApi.unlockVaultWithRecoveryKey(recoveryKey));
    if (ok && requestEpoch === securityEpoch) {
      await get().checkSecurityState({ relockIfPinEnabled: false });
      if (requestEpoch !== securityEpoch) return false;
      if (get().recoveryKeyPending) {
        set({ pendingRecoveryKey: recoveryKey });
      } else {
        get().triggerFirstUnlockSyncIfNeeded();
      }
      return true;
    }
    return false;
  },

  migrateLegacyDatabase: async (password: string) => {
    const requestEpoch = securityEpoch;
    const recoveryKey = await vaultTransition(() => tauriApi.migrateLegacyDatabase(password));
    if (requestEpoch !== securityEpoch) throw new Error('保险库会话已变化，请重新解锁完成恢复密钥确认');
    set({
      vaultStatus: 'unlocked',
      isLocked: true,
      isUiLocked: false,
      hasEnvelope: true,
      hasPlaintextDb: false,
      recoveryKeyPending: true,
      pendingRecoveryKey: recoveryKey,
    });
    return recoveryKey;
  },

  lockVault: async () => {
    // Invalidate before awaiting IPC so no completion from the previous
    // session can repopulate the stores while the backend closes the vault.
    const requestEpoch = ++securityEpoch;
    activeLockEpoch = requestEpoch;
    closeSensitiveSession();
    useHistoryStore.getState().invalidateSession();
    // Enter the protected UI state synchronously. This closes the window for
    // sensitive views while the backend lock request is still in flight.
    set({ isLocked: true, isUiLocked: false, isVaultLocking: true, vaultLockError: null, pendingRecoveryKey: null, isSyncing: false, hasSyncedAfterStart: false });
    try {
      await vaultTransition(() => tauriApi.lockVault());
    } catch (e) {
      if (requestEpoch !== securityEpoch) return;
      console.error('Error locking vault:', e);
      // Keep the backend status truthful. The frontend cache is already
      // cleared, but the vault may still be open and must be re-queried.
      const errorMessage = e instanceof Error ? e.message : String(e);
      await get().checkSecurityState({ relockIfPinEnabled: false }).catch(() => undefined);
      if (requestEpoch !== securityEpoch) return;
      // Invalidate every response belonging to the failed transition before
      // exposing the explicit recovery choices below.
      securityEpoch++;
      activeLockEpoch = null;
      set({
        // A failed lock must never expose the main view again implicitly. The
        // error screen offers an explicit retry or continue action.
        isLocked: true,
        isUiLocked: false,
        vaultLockError: errorMessage,
        isVaultLocking: false,
      });
      return;
    }
    if (requestEpoch !== securityEpoch) return;
    activeLockEpoch = null;
    const pinRequiresResume = get().hasDpapiWrap && get().pinEnabled;
    set({
      vaultStatus: 'locked',
      isVaultLocking: false,
      pendingRecoveryKey: null,
      isLocked: true,
      isUiLocked: pinRequiresResume,
      pinRequiredOnResume: pinRequiresResume,
      vaultLockError: null,
      hasSyncedAfterStart: false,
    });
  },

  restoreVaultBackup: async (source, password, recoveryKey) => {
    if (activeLockEpoch !== null || get().isLocked) throw new Error('保险库正在更改或已锁定');
    const requestEpoch = ++securityEpoch;
    activeLockEpoch = requestEpoch;
    closeSensitiveSession();
    useHistoryStore.getState().invalidateSession();
    set({ isLocked: true, isUiLocked: false, isVaultRestoring: true, vaultLockError: null,
      pendingRecoveryKey: null, isSyncing: false, hasSyncedAfterStart: false });
    try {
      return await vaultTransition(() => typeof source === 'string'
        ? tauriApi.restoreDatabaseBackupFile(source, password, recoveryKey)
        : tauriApi.restoreDatabaseBackup(source, password, recoveryKey));
    } finally {
      set({ isVaultRestoring: false });
      if (requestEpoch === securityEpoch) {
        activeLockEpoch = null;
        await get().checkSecurityState({ relockIfPinEnabled: false });
      }
    }
  },

  changeVaultPassword: async (oldPassword: string, newPassword: string) => {
    const requestEpoch = securityEpoch;
    await tauriApi.changeVaultPassword(oldPassword, newPassword);
    if (requestEpoch !== securityEpoch) return;
  },

  unlock: async (pin: string) => {
    const requestEpoch = securityEpoch;
    if (activeLockEpoch !== null) return false;
    if (get().vaultStatus === 'locked' && get().hasDpapiWrap) {
      const result = await vaultTransition(() => tauriApi.resumePassiveVault(pin));
      if (requestEpoch !== securityEpoch) return false;
      if (result.unlocked) {
        set({
          isUiLocked: false,
          pinRequiredOnResume: false,
          isLockedOut: false,
          lockoutRemainingSecs: 0,
        });
        await get().checkSecurityState({ relockIfPinEnabled: false });
        if (requestEpoch !== securityEpoch) return false;
        get().triggerFirstUnlockSyncIfNeeded();
        return true;
      }
      set({
        isLockedOut: result.is_locked_out,
        lockoutRemainingSecs: result.lockout_remaining_secs,
      });
      throw new Error(result.error_message || 'PIN 码验证失败');
    }
    try {
      const success = await tauriApi.verifyPin(pin);
      if (success && requestEpoch === securityEpoch) {
        set({
          isLocked: false,
          isUiLocked: false,
          pinRequiredOnResume: false,
          isLockedOut: false,
          lockoutRemainingSecs: 0,
        });
        get().triggerFirstUnlockSyncIfNeeded();
        return true;
      }
      return false;
    } catch (err: any) {
      if (requestEpoch !== securityEpoch) return false;
      const state = await tauriApi.getSecurityState().catch(() => null);
      if (requestEpoch !== securityEpoch) return false;
      if (state) {
        set({
          isLockedOut: state.is_locked_out,
          lockoutRemainingSecs: state.lockout_remaining_secs,
        });
      }
      throw err;
    }
  },

  unlockWithRecoveryKey: async (recoveryKey: string) => {
    const requestEpoch = securityEpoch;
    if (activeLockEpoch !== null) return false;
    const success = await tauriApi.verifyRecoveryKey(recoveryKey);
    if (success && requestEpoch === securityEpoch) {
      set({
        isLocked: false,
        isUiLocked: false,
        isLockedOut: false,
        lockoutRemainingSecs: 0,
      });
      get().triggerFirstUnlockSyncIfNeeded();
      return true;
    }
    return false;
  },

  resetPinWithRecoveryKey: async (recoveryKey: string, newPin: string) => {
    const requestEpoch = securityEpoch;
    await vaultTransition(() => tauriApi.resetPinWithRecoveryKey(recoveryKey, newPin));
    if (requestEpoch !== securityEpoch) return;
    set({
      pinEnabled: true,
      pinRequiredOnResume: false,
      isLocked: false,
      isUiLocked: false,
      isLockedOut: false,
      lockoutRemainingSecs: 0,
    });
    await get().checkSecurityState({ relockIfPinEnabled: false });
    if (requestEpoch !== securityEpoch) return;
    get().triggerFirstUnlockSyncIfNeeded();
  },

  lock: () => {
    if (activeLockEpoch !== null) return;
    if (get().vaultStatus !== 'unlocked') {
      securityEpoch++;
      useHistoryStore.getState().invalidateSession();
      set({ isLocked: true, isSyncing: false, lastSyncTime: null, lastSyncCount: 0, hasSyncedAfterStart: false });
    } else if (get().pinEnabled) {
      securityEpoch++;
      useHistoryStore.getState().invalidateSession();
      set({ isLocked: true, isUiLocked: true, isSyncing: false, lastSyncTime: null, lastSyncCount: 0, hasSyncedAfterStart: false });
    }
  },

  setOrChangePin: async (oldPin: string | null, newPin: string) => {
    const requestEpoch = securityEpoch;
    const recoveryKey = await tauriApi.setOrChangePin(oldPin, newPin);
    if (requestEpoch !== securityEpoch) return recoveryKey;
    set({ pinEnabled: true, isLocked: false, isUiLocked: false });
    await get().checkSecurityState({ relockIfPinEnabled: false });
    if (requestEpoch !== securityEpoch) return recoveryKey;
    return recoveryKey;
  },

  disablePin: async (currentPin: string) => {
    const requestEpoch = securityEpoch;
    await tauriApi.disablePin(currentPin);
    if (requestEpoch !== securityEpoch) return;
    set({ pinEnabled: false, isLocked: false, isUiLocked: false });
    await get().checkSecurityState({ relockIfPinEnabled: false });
    if (requestEpoch !== securityEpoch) return;
  },

  updateOptions: async (autoLockMinutes: number, lockOnMinimize: boolean, lockOnSleep: boolean) => {
    const requestEpoch = securityEpoch;
    await tauriApi.updateSecurityOptions(autoLockMinutes, lockOnMinimize, lockOnSleep);
    if (requestEpoch !== securityEpoch) return;
    set({
      autoLockMinutes,
      lockOnMinimize,
      lockOnSleep,
    });
  },

  triggerFirstUnlockSyncIfNeeded: async () => {
    if (get().hasSyncedAfterStart || get().isLocked || get().vaultStatus !== 'unlocked') return;
    const requestEpoch = securityEpoch;
    set({ hasSyncedAfterStart: true });
    console.info('[AutoSync] Triggering first-unlock auto-sync with smooth deferral...');
    setTimeout(() => {
      if (requestEpoch !== securityEpoch) return;
      get()
        .triggerSyncAll({ silentIfBusy: true })
        .catch((e) => {
          console.error('[AutoSync] Background initial sync failed:', e);
        });
    }, 1200);
  },

  recordSyncResults: (results) => {
    if (get().isLocked || get().vaultStatus !== 'unlocked') return;
    const summary = describeSyncResults(results);
    set({
      lastSyncResults: results,
      lastSyncError: summary.failed.length || summary.rejected ? summary.message : null,
      lastSyncCount: summary.inserted,
      ...(summary.succeeded > 0 ? { lastSyncTime: Date.now() } : {}),
    });
  },

  refreshSyncStatus: async () => {
    if (get().isLocked || get().vaultStatus !== 'unlocked') return;
    const requestEpoch = securityEpoch;
    try {
      const jobs = await tauriApi.getRecentImportJobs(50);
      if (requestEpoch !== securityEpoch || get().isLocked) return;
      const latest = new Map<number, typeof jobs[number]>();
      for (const job of jobs) if (job.source_id != null && !latest.has(job.source_id)) latest.set(job.source_id, job);
      const failed = [...latest.values()].filter((job) => job.status === 'failed');
      set({ lastSyncError: failed.length ? failed.map((job) => `${job.browser || ''} / ${job.profile || ''}: ${job.error || '同步失败'}`).join('；') : null });
    } catch (error) {
      if (requestEpoch === securityEpoch && !get().isLocked) console.warn('Could not refresh sync status:', error);
    }
  },

  triggerSyncAll: async (options) => {
    if (get().isSyncing || get().isLocked || get().vaultStatus !== 'unlocked') return 0;
    const requestEpoch = securityEpoch;
    set({ isSyncing: true });
    try {
      const results = await tauriApi.syncAll();
      if (requestEpoch !== securityEpoch) return 0;
      const summary = describeSyncResults(results);
      get().recordSyncResults(results);
      set({ isSyncing: false });
      if (summary.inserted > 0 || useHistoryStore.getState().items.length === 0) {
        useHistoryStore.getState().fetchHistory(true).catch(console.error);
      }
      if (summary.allFailed) throw new Error(summary.message);
      return summary.inserted;
    } catch (error) {
      if (requestEpoch !== securityEpoch) return 0;
      const message = error instanceof Error ? error.message : String(error);
      if (options?.silentIfBusy && message.includes('已有同步任务正在执行')) {
        set({ isSyncing: false });
        void get().refreshSyncStatus();
        return 0;
      }
      set({ isSyncing: false, lastSyncError: message });
      throw error;
    }
  },
}));

// Component-local AI, analytics and quick-search requests use the same response
// barrier as persistent stores, including across a subsequent unlock.
useSecurityStore.subscribe((state) => {
  if (state.isLocked || state.vaultStatus !== 'unlocked') {
    closeSensitiveSession();
    if (state.lastSyncResults.length || state.lastSyncError) useSecurityStore.setState({ lastSyncResults: [], lastSyncError: null });
  }
  else openSensitiveSession();
});

if (typeof window !== 'undefined') {
  window.addEventListener('browsory://vault-locked', () => {
    if (activeLockEpoch === null) securityEpoch++;
    closeSensitiveSession();
    useHistoryStore.getState().invalidateSession();
    useSecurityStore.setState({
      pendingRecoveryKey: null,
      hasSyncedAfterStart: false,
      isLocked: true,
      isUiLocked: false,
      isSyncing: false,
    });
    // An access rejection may arrive while Rust is still draining. Query the
    // actual state instead of claiming that the connection is already closed.
    void useSecurityStore.getState().checkSecurityState();
  });
}
