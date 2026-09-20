import React, { useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { Sidebar } from './components/Layout/Sidebar';
import { RecallHomePage } from './pages/Home/RecallHomePage';
import { HistoryPage } from './pages/History/HistoryPage';
import { AnalyticsPage } from './pages/Analytics/AnalyticsPage';
import { SourcesPage } from './pages/Sources/SourcesPage';
import { AIPage } from './pages/AI/AIPage';
import { PrivacyPage } from './pages/Privacy/PrivacyPage';
import { SettingsPage } from './pages/Settings/SettingsPage';
import { RunningModal } from './components/Layout/RunningModal';
import { ToastContainer } from './components/Common/Toast';
import { LockScreen } from './components/Layout/LockScreen';
import { QuickSearchModal } from './components/QuickSearch/QuickSearchModal';
import { OnboardingWizard } from './components/Onboarding/OnboardingWizard';
import { useAppStore } from './stores/useAppStore';
import { useSecurityStore } from './stores/useSecurityStore';
import { useHistoryStore } from './stores/useHistoryStore';

export const App: React.FC = () => {
  const { currentTab, setCurrentTab, setTheme, setLanguage } = useAppStore();
  const {
    isInitialized,
    isLocked,
    pinEnabled,
    autoLockMinutes,
    lockOnMinimize,
    lockOnSleep,
    checkSecurityState,
    lock,
    triggerSyncAll,
  } = useSecurityStore();

  const [isQuickSearchOpen, setIsQuickSearchOpen] = useState(false);
  const [showOnboarding, setShowOnboarding] = useState(false);
  const lastActivityRef = useRef<number>(Date.now());

  useEffect(() => {
    // 1. Restore persisted settings
    const savedTheme = localStorage.getItem('app_theme');
    if (savedTheme) {
      setTheme(savedTheme as any);
    }
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

  // 4. Minimize / Sleep / Blur handler
  useEffect(() => {
    const handleVisibilityChange = () => {
      if (document.visibilityState === 'hidden' && lockOnMinimize && pinEnabled) {
        lock();
      }
    };

    const handleWindowBlur = () => {
      if (lockOnMinimize && pinEnabled) {
        lock();
      }
    };

    // Keyboard shortcut Cmd/Ctrl + L (Lock), Cmd/Ctrl + K (Quick Search), Ctrl + Shift + Space (Spotlight)
    const handleGlobalKeyDown = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'l') {
        e.preventDefault();
        if (pinEnabled) {
          lock();
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
    window.addEventListener('blur', handleWindowBlur);
    window.addEventListener('keydown', handleGlobalKeyDown);

    return () => {
      document.removeEventListener('visibilitychange', handleVisibilityChange);
      window.removeEventListener('blur', handleWindowBlur);
      window.removeEventListener('keydown', handleGlobalKeyDown);
    };
  }, [pinEnabled, lockOnMinimize, isLocked, lock]);

  // 5. System Sleep / Wake Heartbeat Detection
  useEffect(() => {
    let lastHeartbeat = Date.now();
    const interval = setInterval(() => {
      const now = Date.now();
      const diff = now - lastHeartbeat;
      lastHeartbeat = now;

      // If more than 3500ms elapsed for a 1000ms timer, system was asleep / suspended
      if (diff > 3500) {
        if (pinEnabled && lockOnSleep && !isLocked) {
          console.info(`[Security] System wake detected (time jump: ${diff}ms). Locking application.`);
          lock();
        }
      }
    }, 1000);

    return () => clearInterval(interval);
  }, [pinEnabled, lockOnSleep, isLocked, lock]);

  // 6. Listen to Tauri Tray & Window events
  useEffect(() => {
    const handleOpenQuickSearch = () => {
      if (!isLocked) {
        setIsQuickSearchOpen(true);
      }
    };

    window.addEventListener('browsory:open-quick-search', handleOpenQuickSearch);

    let unlistenSearch: (() => void) | undefined;
    let unlistenSync: (() => void) | undefined;
    let unlistenLock: (() => void) | undefined;

    listen('browsory://open-quick-search', () => {
      handleOpenQuickSearch();
    }).then((un) => {
      unlistenSearch = un;
    });

    listen('browsory://sync-now', () => {
      triggerSyncAll();
    }).then((un) => {
      unlistenSync = un;
    });

    listen('browsory://lock', () => {
      if (pinEnabled) {
        lock();
      }
    }).then((un) => {
      unlistenLock = un;
    });

    return () => {
      window.removeEventListener('browsory:open-quick-search', handleOpenQuickSearch);
      if (unlistenSearch) unlistenSearch();
      if (unlistenSync) unlistenSync();
      if (unlistenLock) unlistenLock();
    };
  }, [isLocked, pinEnabled, lock, triggerSyncAll]);

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
              Browsory 数据已安全锁定
            </div>
          </div>
        ) : (
          <>
            {currentTab === 'home' && <RecallHomePage />}
            {currentTab === 'history' && <HistoryPage />}
            {currentTab === 'analytics' && <AnalyticsPage />}
            {currentTab === 'sources' && <SourcesPage />}
            {currentTab === 'ai' && <AIPage />}
            {currentTab === 'privacy' && <PrivacyPage />}
            {currentTab === 'settings' && <SettingsPage />}
          </>
        )}
      </main>

      {/* Security Lock Screen Overlay */}
      {isLocked && isInitialized && <LockScreen />}

      {/* Global Modals & Notifications */}
      <OnboardingWizard
        isOpen={showOnboarding && !isLocked}
        onComplete={() => setShowOnboarding(false)}
      />
      <QuickSearchModal
        isOpen={isQuickSearchOpen && !isLocked}
        onClose={() => setIsQuickSearchOpen(false)}
        onOpenInApp={(item) => {
          setCurrentTab('history');
          useHistoryStore.getState().openDetail(item.id);
        }}
      />
      <RunningModal />
      <ToastContainer />
    </div>
  );
};

export default App;

