import { create } from 'zustand';
import { tauriApi } from '../services/tauri';
import { SecurityState } from '../types';
import { useHistoryStore } from './useHistoryStore';

const isScreenshotPreview =
  typeof window !== 'undefined' && new URLSearchParams(window.location.search).has('preview');

interface SecurityStore {
  isInitialized: boolean;
  isLocked: boolean;
  pinEnabled: boolean;
  autoLockMinutes: number;
  lockOnMinimize: boolean;
  lockOnSleep: boolean;
  isLockedOut: boolean;
  lockoutRemainingSecs: number;
  hasSyncedAfterStart: boolean;
  isSyncing: boolean;
  lastSyncTime: number | null;
  lastSyncCount: number;

  checkSecurityState: (options?: { relockIfPinEnabled?: boolean }) => Promise<void>;
  unlock: (pin: string) => Promise<boolean>;
  unlockWithRecoveryKey: (recoveryKey: string) => Promise<boolean>;
  resetPinWithRecoveryKey: (recoveryKey: string, newPin: string) => Promise<void>;
  lock: () => void;
  setOrChangePin: (oldPin: string | null, newPin: string) => Promise<string | null>;
  disablePin: (currentPin: string) => Promise<void>;
  updateOptions: (autoLockMinutes: number, lockOnMinimize: boolean, lockOnSleep: boolean) => Promise<void>;
  triggerFirstUnlockSyncIfNeeded: () => Promise<void>;
  triggerSyncAll: () => Promise<number>;
}

export const useSecurityStore = create<SecurityStore>((set, get) => ({
  isInitialized: isScreenshotPreview,
  isLocked: !isScreenshotPreview, // Fail closed outside the local preview.
  pinEnabled: false,
  autoLockMinutes: 15,
  lockOnMinimize: false,
  lockOnSleep: true,
  isLockedOut: false,
  lockoutRemainingSecs: 0,
  hasSyncedAfterStart: false,
  isSyncing: false,
  lastSyncTime: null,
  lastSyncCount: 0,

  checkSecurityState: async (options) => {
    if (isScreenshotPreview) return;
    const relockIfPinEnabled = options?.relockIfPinEnabled !== false;
    try {
      const state: SecurityState = await tauriApi.getSecurityState();
      set({
        isInitialized: true,
        pinEnabled: state.pin_enabled,
        autoLockMinutes: state.auto_lock_minutes,
        lockOnMinimize: state.lock_on_minimize,
        lockOnSleep: state.lock_on_sleep,
        isLockedOut: state.is_locked_out,
        lockoutRemainingSecs: state.lockout_remaining_secs,
        // Startup may lock when PIN is on. After enabling/changing PIN, keep the
        // current session unlocked so the recovery key can be shown and saved.
        isLocked: state.pin_enabled
          ? relockIfPinEnabled
            ? true
            : get().isLocked
          : false,
      });

      // If pin is not enabled, auto trigger startup sync once
      if (!state.pin_enabled && !get().hasSyncedAfterStart) {
        get().triggerFirstUnlockSyncIfNeeded();
      }
    } catch (e) {
      console.error('Failed to get security state:', e);
      // Fail-Closed: preserve locked state upon error to prevent security leak
      set({ isInitialized: true, isLocked: true });
    }
  },

  unlock: async (pin: string) => {
    try {
      const success = await tauriApi.verifyPin(pin);
      if (success) {
        set({
          isLocked: false,
          isLockedOut: false,
          lockoutRemainingSecs: 0,
        });
        // Automatic first sync after app start
        get().triggerFirstUnlockSyncIfNeeded();
        return true;
      }
      return false;
    } catch (err: any) {
      // Refresh state on error to capture lockout timer
      const state = await tauriApi.getSecurityState().catch(() => null);
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
    const success = await tauriApi.verifyRecoveryKey(recoveryKey);
    if (success) {
      set({
        isLocked: false,
        isLockedOut: false,
        lockoutRemainingSecs: 0,
      });
      get().triggerFirstUnlockSyncIfNeeded();
      return true;
    }
    return false;
  },

  resetPinWithRecoveryKey: async (recoveryKey: string, newPin: string) => {
    await tauriApi.resetPinWithRecoveryKey(recoveryKey, newPin);
    set({
      pinEnabled: true,
      isLocked: false,
      isLockedOut: false,
      lockoutRemainingSecs: 0,
    });
    await get().checkSecurityState({ relockIfPinEnabled: false });
  },

  lock: () => {
    if (get().pinEnabled) {
      set({ isLocked: true });
    }
  },

  setOrChangePin: async (oldPin: string | null, newPin: string) => {
    const recoveryKey = await tauriApi.setOrChangePin(oldPin, newPin);
    set({ pinEnabled: true, isLocked: false });
    await get().checkSecurityState({ relockIfPinEnabled: false });
    return recoveryKey;
  },

  disablePin: async (currentPin: string) => {
    await tauriApi.disablePin(currentPin);
    set({ pinEnabled: false, isLocked: false });
    await get().checkSecurityState({ relockIfPinEnabled: false });
  },

  updateOptions: async (autoLockMinutes: number, lockOnMinimize: boolean, lockOnSleep: boolean) => {
    await tauriApi.updateSecurityOptions(autoLockMinutes, lockOnMinimize, lockOnSleep);
    set({
      autoLockMinutes,
      lockOnMinimize,
      lockOnSleep,
    });
  },

  triggerFirstUnlockSyncIfNeeded: async () => {
    if (get().hasSyncedAfterStart) return;
    set({ hasSyncedAfterStart: true });
    console.info('[AutoSync] Triggering first-unlock auto-sync with smooth deferral...');
    // Defer 1200ms to allow immediate first-screen render and zero IPC contention
    setTimeout(() => {
      get()
        .triggerSyncAll()
        .catch((e) => {
          console.error('[AutoSync] Background initial sync failed:', e);
        });
    }, 1200);
  },

  triggerSyncAll: async () => {
    if (get().isSyncing) return 0;
    set({ isSyncing: true });
    try {
      const results = await tauriApi.syncAll();
      const totalNew = results.reduce((acc, r) => acc + r.inserted_count, 0);
      set({
        isSyncing: false,
        lastSyncTime: Date.now(),
        lastSyncCount: totalNew,
      });

      if (totalNew > 0 || useHistoryStore.getState().items.length === 0) {
        useHistoryStore.getState().fetchHistory(true).catch((err) => {
          console.error('[AutoSync] Silent refresh history failed:', err);
        });
      }

      return totalNew;
    } catch (e) {
      console.error('[AutoSync] Sync failed:', e);
      set({ isSyncing: false });
      return 0;
    }
  },
}));
