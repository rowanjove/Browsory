import React, { useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { Sidebar } from './components/Layout/Sidebar';
const RecallHomePage = React.lazy(() => import('./pages/Home/RecallHomePage').then((module) => ({ default: module.RecallHomePage })));
const HistoryPage = React.lazy(() => import('./pages/History/HistoryPage').then((module) => ({ default: module.HistoryPage })));
const AnalyticsPage = React.lazy(() => import('./pages/Analytics/AnalyticsPage').then((module) => ({ default: module.AnalyticsPage })));
const SourcesPage = React.lazy(() => import('./pages/Sources/SourcesPage').then((module) => ({ default: module.SourcesPage })));
const AIPage = React.lazy(() => import('./pages/AI/AIPage').then((module) => ({ default: module.AIPage })));
const PrivacyPage = React.lazy(() => import('./pages/Privacy/PrivacyPage').then((module) => ({ default: module.PrivacyPage })));
const SettingsPage = React.lazy(() => import('./pages/Settings/SettingsPage').then((module) => ({ default: module.SettingsPage })));
import { RunningModal } from './components/Layout/RunningModal';
import { ToastContainer } from './components/Common/Toast';
import { LockScreen } from './components/Layout/LockScreen';
import { QuickSearchModal } from './components/QuickSearch/QuickSearchModal';
import { OnboardingWizard } from './components/Onboarding/OnboardingWizard';
import { CloseConfirmModal } from './components/Layout/CloseConfirmModal';
import { applyTheme, useAppStore } from './stores/useAppStore';
import { useSecurityStore } from './stores/useSecurityStore';
import { useHistoryStore } from './stores/useHistoryStore';
import { tauriApi } from './services/tauri';

export const App: React.FC = () => {
  const { currentTab, setCurrentTab, setTheme, setLanguage, setAppMeta, t, language } = useAppStore();
  const security = useSecurityStore();
  const {
    isInitialized,
    isLocked,
    pinEnabled,
    hasDpapiWrap,
    autoLockMinutes,
    lockOnMinimize,
    checkSecurityState,
    lock,
    lockVault,
    refreshSyncStatus,
    triggerSyncAll,
  } = security;

  const [isQuickSearchOpen, setIsQuickSearchOpen] = useState(false);
  const [showOnboarding, setShowOnboarding] = useState(false);
  const [showCloseConfirm, setShowCloseConfirm] = useState(false);
  const lastActivityRef = useRef<number>(Date.now());
  const refreshedAfterUnlockRef = useRef(false);

  useEffect(() => {
    if (!isLocked) return;
    setIsQuickSearchOpen(false);
    const clearGlobalSensitiveUi = () => {
      const state = useAppStore.getState();
      if (state.toasts.length || state.runningModal.isOpen) {
        useAppStore.setState({ toasts: [], runningModal: { isOpen: false, browser: '' } });
      }
    };
    clearGlobalSensitiveUi();
    return useAppStore.subscribe(clearGlobalSensitiveUi);
  }, [isLocked]);

  useEffect(() => {
    if (isLocked || !isInitialized) {
      refreshedAfterUnlockRef.current = false;
      return;
    }
    if (refreshedAfterUnlockRef.current) return;
    refreshedAfterUnlockRef.current = true;
    void refreshSyncStatus().catch((error) => {
      console.warn('[Sync] Unable to refresh status after unlock:', error);
    });
  }, [isInitialized, isLocked, refreshSyncStatus]);

  useEffect(() => {
    // 1. Restore persisted settings
    const savedTheme = localStorage.getItem('app_theme');
    setTheme(savedTheme === 'light' || savedTheme === 'dark' || savedTheme === 'system' ? savedTheme : 'system');
    const savedLang = localStorage.getItem('app_language');
    if (savedLang) {
      setLanguage(savedLang as any);
    }

    // Check first launch onboarding
    const hasOnboarded = localStorage.getItem('browsory_onboarded_v1');
    if (!hasOnboarded) {
      setShowOnboarding(true);
    }

    // 2. Initialize security & check lock status
    checkSecurityState();
    tauriApi
      .getAppInfo()
      .then((info) => setAppMeta(info.version, info.is_portable))
      .catch(console.error);
  }, []);

  useEffect(() => {
    if (typeof window === 'undefined') return;
    if (typeof window.matchMedia !== 'function') return;
    const media = window.matchMedia('(prefers-color-scheme: dark)');
    const handleSystemThemeChange = () => {
      if (useAppStore.getState().theme === 'system') applyTheme('system');
    };
    media.addEventListener?.('change', handleSystemThemeChange);
    return () => media.removeEventListener?.('change', handleSystemThemeChange);
  }, []);

  // 3. User Activity Tracker for Auto-Lock
  useEffect(() => {
    const handleActivity = () => {
      lastActivityRef.current = Date.now();
    };

    const events = ['mousemove', 'mousedown', 'keydown', 'scroll', 'touchstart'];
    events.forEach((ev) => window.addEventListener(ev, handleActivity, { passive: true }));

    const interval = setInterval(() => {
      if (pinEnabled && !isLocked && autoLockMinutes > 0) {
        const idleMs = Date.now() - lastActivityRef.current;
        if (idleMs >= autoLockMinutes * 60 * 1000) {
          console.info(`[Security] Application locked after ${autoLockMinutes}m idle.`);
          lock();
        }
      }
    }, 10000);

    return () => {
      events.forEach((ev) => window.removeEventListener(ev, handleActivity));
      clearInterval(interval);
    };
  }, [pinEnabled, isLocked, autoLockMinutes, lock]);

  // 4. Visibility and global shortcuts. Window blur is deliberately not used:
  // switching to another app is not the same as minimizing this window.
  useEffect(() => {
    const handleVisibilityChange = () => {
      if (document.visibilityState === 'hidden' && lockOnMinimize && pinEnabled) {
        lock();
      }
    };

    // Keyboard shortcut Cmd/Ctrl + L (Lock), Cmd/Ctrl + K (Quick Search), Ctrl + Shift + Space (Spotlight)
    const handleGlobalKeyDown = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'l') {
        e.preventDefault();
        if (!hasDpapiWrap || pinEnabled) {
          void lockVault();
        } else {
          const appStore = useAppStore.getState();
          appStore.showToast(
            appStore.language === 'en-US'
              ? 'Background encryption is active; set a 4-digit PIN in Privacy Settings to enable manual lock'
              : '已启用后台静默加密；请先在隐私安全页设置 4 位 PIN 码再主动锁屏',
            'info'
          );
          appStore.setCurrentTab('privacy');
        }
      } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        if (!isLocked) {
          setIsQuickSearchOpen((prev) => !prev);
        }
      } else if (e.ctrlKey && e.shiftKey && (e.code === 'Space' || e.key === ' ')) {
        e.preventDefault();
        if (!isLocked) {
          setIsQuickSearchOpen((prev) => !prev);
        }
      }
    };

    document.addEventListener('visibilitychange', handleVisibilityChange);
    window.addEventListener('keydown', handleGlobalKeyDown);

    return () => {
      document.removeEventListener('visibilitychange', handleVisibilityChange);
      window.removeEventListener('keydown', handleGlobalKeyDown);
    };
  }, [lockOnMinimize, pinEnabled, hasDpapiWrap, isLocked, lock, lockVault]);

  // 6. Listen to Tauri Tray & Window events
  useEffect(() => {
    const handleOpenQuickSearch = () => {
      if (!isLocked) {
        setIsQuickSearchOpen(true);
      }
    };

    window.addEventListener('browsory:open-quick-search', handleOpenQuickSearch);

    let disposed = false;
    const listeners = new Set<() => void>();
    const rememberListener = (unlisten: () => void) => {
      if (disposed) unlisten();
      else listeners.add(unlisten);
    };

    listen('browsory://open-quick-search', () => {
      handleOpenQuickSearch();
    }).then(rememberListener).catch(() => { /* Native events are unavailable in browser preview. */ });

    listen('browsory://sync-now', () => {
      triggerSyncAll();
    }).then(rememberListener).catch(() => { /* Native events are unavailable in browser preview. */ });

    listen('browsory://sync-status', () => {
      if (useSecurityStore.getState().isLocked) return;
      void refreshSyncStatus().catch((error) => {
        console.warn('[Sync] Unable to refresh status:', error);
      });
    }).then(rememberListener).catch(() => { /* Native events are unavailable in browser preview. */ });

    listen('browsory://auto-sync-updated', () => {
      if (useSecurityStore.getState().isLocked) return;
      void refreshSyncStatus().catch((error) => {
        console.warn('[AutoSync] Unable to refresh status:', error);
      });
    }).then(rememberListener).catch(() => { /* Native events are unavailable in browser preview. */ });

    listen('browsory://lock', () => {
      void lockVault();
    }).then(rememberListener).catch(() => { /* Native events are unavailable in browser preview. */ });

    listen('browsory://power', () => {
      void useSecurityStore.getState().checkSecurityState();
    }).then(rememberListener).catch(() => { /* Native notifications only. */ });

    listen('browsory://close-requested', () => {
      setShowCloseConfirm(true);
    }).then(rememberListener).catch(() => { /* Native events are unavailable in browser preview. */ });

    return () => {
      window.removeEventListener('browsory:open-quick-search', handleOpenQuickSearch);
      disposed = true;
      for (const unlisten of listeners) unlisten();
      listeners.clear();
    };
  }, [isLocked, lockVault, refreshSyncStatus, triggerSyncAll]);

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-white dark:bg-slate-900 text-slate-900 dark:text-slate-100">
      <Sidebar />
      <main className="flex-1 flex flex-col h-full overflow-hidden relative">
        {/*
          Security Policy:
          When locked, sensitive components (History, Analytics, AI, Sources, Settings)
          MUST BE UNMOUNTED from the DOM tree, not merely hidden with CSS blur.
        */}
        {isLocked ? (
          <div className="flex-1 flex items-center justify-center bg-slate-50 dark:bg-slate-950/40 select-none">
            <div className="text-center text-slate-400 dark:text-slate-600 text-xs">
              {language === 'en-US' ? 'Browsory data is safely locked' : 'Browsory 数据已安全锁定'}
            </div>
          </div>
        ) : (
          <React.Suspense fallback={<div role="status" className="flex-1 flex items-center justify-center text-sm text-slate-500">{t('history.loading')}</div>}>
            {currentTab === 'home' && <RecallHomePage />}
            {currentTab === 'history' && <HistoryPage />}
            {currentTab === 'analytics' && <AnalyticsPage />}
            {currentTab === 'sources' && <SourcesPage />}
            {currentTab === 'ai' && <AIPage />}
            {currentTab === 'privacy' && <PrivacyPage />}
            {currentTab === 'settings' && <SettingsPage />}
          </React.Suspense>
        )}
      </main>

      {/* Security Lock Screen Overlay */}
      {isLocked && isInitialized && <LockScreen />}

      {/* Global Modals & Notifications */}
      <OnboardingWizard
        isOpen={showOnboarding && !isLocked}
        onComplete={() => setShowOnboarding(false)}
      />
      {!isLocked && <QuickSearchModal
        isOpen={isQuickSearchOpen && !isLocked}
        onClose={() => setIsQuickSearchOpen(false)}
        onOpenInApp={(item) => {
          setCurrentTab('history');
          useHistoryStore.getState().openDetail(item.id);
        }}
      />}
      {!isLocked && <RunningModal />}
      <CloseConfirmModal
        isOpen={showCloseConfirm}
        onClose={() => setShowCloseConfirm(false)}
      />
      {!isLocked && <ToastContainer />}
    </div>
  );
};

export default App;

