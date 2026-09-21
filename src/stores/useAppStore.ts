import { create } from 'zustand';
import zhCN from '../locales/zh-CN.json';
import enUS from '../locales/en-US.json';
import { NavTab } from '../types';

type Language = 'zh-CN' | 'en-US';
type Theme = 'light' | 'dark' | 'system';

interface RunningModalState {
  isOpen: boolean;
  browser: string;
  sourceId?: number;
}

interface ToastMessage {
  id: string;
  type: 'info' | 'success' | 'warning' | 'error';
  content: string;
}

interface AppStore {
  currentTab: NavTab;
  language: Language;
  theme: Theme;
  appVersion: string;
  isPortable: boolean;
  runningModal: RunningModalState;
  toasts: ToastMessage[];
  isChangelogOpen: boolean;

  setAppMeta: (version: string, isPortable: boolean) => void;
  setCurrentTab: (tab: NavTab) => void;
  setLanguage: (lang: Language) => void;
  setTheme: (theme: Theme) => void;
  openRunningModal: (browser: string, sourceId?: number) => void;
  closeRunningModal: () => void;
  openChangelog: () => void;
  closeChangelog: () => void;
  showToast: (content: string, type?: 'info' | 'success' | 'warning' | 'error') => void;
  removeToast: (id: string) => void;
  t: (keyPath: string, params?: Record<string, string | number>) => string;
}

const locales = {
  'zh-CN': zhCN,
  'en-US': enUS,
};

const previewParams = typeof window !== 'undefined' ? new URLSearchParams(window.location.search) : null;
const previewLanguage: Language = previewParams?.get('lang') === 'en-US' ? 'en-US' : 'zh-CN';
const previewTab = previewParams?.get('tab');
const initialTab: NavTab =
  previewTab === 'history' || previewTab === 'analytics' || previewTab === 'sources' || previewTab === 'ai' || previewTab === 'settings'
    ? (previewTab as NavTab)
    : 'home';

export const useAppStore = create<AppStore>((set, get) => ({
  currentTab: initialTab,
  language: previewLanguage,
  theme: 'system',
  appVersion: '0.1.2',
  isPortable: false,
  runningModal: {
    isOpen: false,
    browser: '',
  },
  toasts: [],
  isChangelogOpen: false,

  setAppMeta: (version, isPortable) => set({ appVersion: version, isPortable }),
  setCurrentTab: (tab) => set({ currentTab: tab }),

  setLanguage: (lang) => {
    set({ language: lang });
    localStorage.setItem('app_language', lang);
  },

  setTheme: (theme) => {
    set({ theme });
    localStorage.setItem('app_theme', theme);
    applyTheme(theme);
  },

  openRunningModal: (browser, sourceId) =>
    set({
      runningModal: {
        isOpen: true,
        browser,
        sourceId,
      },
    }),

  closeRunningModal: () =>
    set({
      runningModal: {
        isOpen: false,
        browser: '',
      },
    }),

  openChangelog: () => set({ isChangelogOpen: true }),
  closeChangelog: () => set({ isChangelogOpen: false }),

  showToast: (content, type = 'info') => {
    const id = Math.random().toString(36).substring(2, 9);
    set((state) => ({
      toasts: [...state.toasts, { id, type, content }],
    }));
    setTimeout(() => {
      get().removeToast(id);
    }, 4000);
  },

  removeToast: (id) =>
    set((state) => ({
      toasts: state.toasts.filter((t) => t.id !== id),
    })),

  t: (keyPath, params) => {
    const lang = get().language;
    const currentDict = locales[lang] || locales['zh-CN'];

    const keys = keyPath.split('.');
    let value: any = currentDict;
    for (const k of keys) {
      if (value && typeof value === 'object' && k in value) {
        value = value[k];
      } else {
        return keyPath;
      }
    }

    if (typeof value !== 'string') {
      return keyPath;
    }

    let res = value;
    if (params) {
      for (const [k, v] of Object.entries(params)) {
        res = res.replace(new RegExp(`{{${k}}}`, 'g'), String(v));
      }
    }
    return res;
  },
}));

function applyTheme(theme: Theme) {
  const root = document.documentElement;
  const isDark =
    theme === 'dark' ||
    (theme === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches);

  if (isDark) {
    root.classList.add('dark');
  } else {
    root.classList.remove('dark');
  }
}
