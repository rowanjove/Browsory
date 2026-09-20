import { create } from 'zustand';
import { tauriApi } from '../services/tauri';
import { LicenseInfo } from '../types';

interface LicenseState {
  license: LicenseInfo | null;
  loading: boolean;
  error: string | null;
  fetchLicense: () => Promise<void>;
  activate: (key: string) => Promise<boolean>;
  deactivate: () => Promise<void>;
}

export const useLicenseStore = create<LicenseState>((set, get) => ({
  license: null,
  loading: false,
  error: null,

  fetchLicense: async () => {
    try {
      set({ loading: true, error: null });
      const info = await tauriApi.getLicenseInfo();
      set({ license: info, loading: false });
    } catch (err: any) {
      console.error('Failed to load license status:', err);
      set({ loading: false, error: err?.toString() || '加载授权状态失败' });
    }
  },

  activate: async (key: string) => {
    try {
      set({ loading: true, error: null });
      await tauriApi.activateLicense(key);
      await get().fetchLicense();
      return true;
    } catch (err: any) {
      console.error('License activation failed:', err);
      set({ loading: false, error: err?.toString() || '授权激活失败' });
      return false;
    }
  },

  deactivate: async () => {
    try {
      set({ loading: true, error: null });
      await tauriApi.deactivateLicense();
      await get().fetchLicense();
    } catch (err: any) {
      console.error('License deactivation failed:', err);
      set({ loading: false, error: err?.toString() || '移除授权失败' });
    }
  },
}));
