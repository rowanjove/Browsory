import React, { useState } from 'react';
import { Compass, History, BarChart3, Database, Sparkles, Settings, Lock, Activity, ShieldCheck, AlertTriangle } from 'lucide-react';
import { useAppStore } from '../../stores/useAppStore';
import { useSecurityStore } from '../../stores/useSecurityStore';
import { NavTab } from '../../types';
import { SyncCenterModal } from './SyncCenterModal';
import { JobManagerModal } from './JobManagerModal';
import { ChangelogModal } from './ChangelogModal';

export const Sidebar: React.FC = () => {
  const { currentTab, setCurrentTab, openChangelog, closeChangelog, isChangelogOpen, showToast, t, appVersion, language } = useAppStore();
  const ui = (zh: string, en: string) => (language === 'en-US' ? en : zh);
  const security = useSecurityStore();
  const { isLocked, lockVault, isSyncing, lastSyncTime, lastSyncCount, lastSyncError } = security;
  const [isSyncCenterOpen, setIsSyncCenterOpen] = useState(false);
  const [isJobManagerOpen, setIsJobManagerOpen] = useState(false);

  React.useEffect(() => {
    if (!isLocked) return;
    setIsSyncCenterOpen(false);
    setIsJobManagerOpen(false);
    if (isChangelogOpen) closeChangelog();
  }, [closeChangelog, isChangelogOpen, isLocked]);

  const navItems: { id: NavTab; label: string; icon: React.ReactNode }[] = [
    { id: 'home', label: t('nav.home'), icon: <Compass className="w-4 h-4" /> },
    { id: 'history', label: t('nav.history'), icon: <History className="w-4 h-4" /> },
    { id: 'analytics', label: t('nav.analytics'), icon: <BarChart3 className="w-4 h-4" /> },
    { id: 'sources', label: t('nav.sources'), icon: <Database className="w-4 h-4" /> },
    { id: 'ai', label: t('nav.ai'), icon: <Sparkles className="w-4 h-4" /> },
    { id: 'privacy', label: t('nav.privacy'), icon: <ShieldCheck className="w-4 h-4" /> },
  ];

  return (
    <aside className="w-48 bg-slate-50 dark:bg-slate-900 border-r border-slate-200 dark:border-slate-800 flex flex-col justify-between select-none shrink-0 h-full">
      <div className="flex flex-col">
        {/* Workspace Archive Header */}
        <div className="h-10 px-3 border-b border-slate-200 dark:border-slate-800 flex items-center justify-between gap-2">
          <div className="flex items-center gap-2 min-w-0">
            <span className="w-2 h-2 rounded-full bg-emerald-500 shrink-0" />
            <span className="text-xs font-semibold tracking-tight text-slate-700 dark:text-slate-200 truncate">
              {ui('本地记忆库', 'Local Archive')}
            </span>
          </div>
          <button
            type="button"
            onClick={openChangelog}
            className="text-[9px] px-1.5 py-0.5 rounded bg-slate-200/70 hover:bg-blue-50 hover:text-blue-600 dark:bg-slate-800 dark:hover:bg-blue-950/60 dark:hover:text-blue-400 text-slate-500 font-mono shrink-0 cursor-pointer transition border border-transparent hover:border-blue-200 dark:hover:border-blue-800"
            title={ui('点击查看版本更新说明', 'View release notes')}
          >
            v{appVersion}
          </button>
        </div>

        {/* Nav Links */}
        <nav className="p-2 flex flex-col gap-0.5">
          {navItems.map((item) => {
            const isActive = currentTab === item.id;
            return (
              <button
                key={item.id}
                onClick={() => setCurrentTab(item.id)}
                className={`w-full flex items-center gap-2.5 px-3 py-2 rounded text-xs transition-colors ${
                  isActive
                    ? 'bg-blue-600 text-white font-medium shadow-sm'
                    : 'text-slate-600 dark:text-slate-400 hover:bg-slate-200/60 dark:hover:bg-slate-800 hover:text-slate-900 dark:hover:text-slate-100'
                }`}
              >
                {item.icon}
                <span>{item.label}</span>
              </button>
            );
          })}
        </nav>
      </div>

      {/* Footer Info with Sync Status & Lock & Theme Toggle */}
      <div className="p-2 border-t border-slate-200 dark:border-slate-800 flex flex-col gap-2">
        {/* Sync Status Button */}
        <button
          onClick={() => {
            if (!isLocked) setIsSyncCenterOpen(true);
          }}
          disabled={isLocked}
          className="w-full flex items-center justify-between px-2.5 py-1.5 rounded-lg bg-slate-100 dark:bg-slate-800/60 hover:bg-slate-200/70 dark:hover:bg-slate-800 transition text-[11px] text-slate-600 dark:text-slate-300"
          title={ui('点击打开数据同步中心', 'Open sync center')}
        >
          <div className="flex items-center gap-2 min-w-0 pr-1">
            <span className="relative flex h-2.5 w-2.5 shrink-0 items-center justify-center">
              {isSyncing ? (
                <>
                  <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-amber-400 opacity-75" />
                  <span className="relative inline-flex rounded-full h-1.5 w-1.5 bg-amber-500" />
                </>
              ) : (
                <span className="inline-flex rounded-full h-1.5 w-1.5 bg-emerald-500" />
              )}
            </span>
            <span className={`truncate ${lastSyncError ? 'text-rose-600 dark:text-rose-400 font-medium' : ''}`}>
              {lastSyncError
                ? t('settings.syncFailed')
                : isSyncing
                ? ui('正在同步...', 'Syncing...')
                : lastSyncTime
                ? ui(`已同步 ${new Date(lastSyncTime).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}`, `Synced ${new Date(lastSyncTime).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}`)
                : ui('同步就绪', 'Sync ready')}
            </span>
          </div>
          {lastSyncError ? (
              <AlertTriangle className="w-3.5 h-3.5 shrink-0 text-rose-500" aria-label={t('settings.syncFailed')} />
          ) : lastSyncCount > 0 && !isSyncing ? (
            <span className="text-[10px] font-mono font-semibold text-emerald-600 dark:text-emerald-400">
              +{lastSyncCount}
            </span>
          ) : null}
        </button>

        {/* Bottom utility bar */}
        <div className="flex items-center justify-between gap-2 px-1 text-[11px] text-slate-400">
          <div className="flex items-center gap-1 shrink-0">
            <button
              onClick={() => {
                if (!isLocked) setIsJobManagerOpen(true);
              }}
              disabled={isLocked}
              className="p-1.5 rounded hover:bg-slate-200 dark:hover:bg-slate-800 text-slate-500 hover:text-slate-800 dark:hover:text-slate-200 transition-colors shrink-0 cursor-pointer"
              title={ui('后台任务与队列中心', 'Background jobs and queue')}
            >
              <Activity className="w-3.5 h-3.5 text-slate-500 hover:text-blue-500 transition-colors" />
            </button>

            <button
              onClick={() => {
                if (!security.hasDpapiWrap || security.pinEnabled) {
                  void lockVault();
                } else {
                  showToast(
                    ui('已启用后台静默加密；请先设置 4 位 PIN 码以使用主动锁屏', 'Background encryption is active; set a 4-digit PIN first to lock manually'),
                    'info'
                  );
                  setCurrentTab('privacy');
                }
              }}
              className="p-1.5 rounded hover:bg-slate-200 dark:hover:bg-slate-800 text-slate-500 hover:text-slate-800 dark:hover:text-slate-200 transition-colors shrink-0 cursor-pointer"
              title={`${t('settings.vaultLock')} (Cmd/Ctrl + L)`}
              aria-label={t('settings.vaultLock')}
            >
              <Lock className="w-3.5 h-3.5 text-slate-500 hover:text-amber-500 transition-colors" />
            </button>
          </div>

          <button
            onClick={() => setCurrentTab('settings')}
            className={`flex items-center gap-1.5 px-2.5 py-1 rounded transition-colors cursor-pointer text-xs whitespace-nowrap shrink-0 ${
              currentTab === 'settings'
                ? 'bg-blue-600 text-white font-medium shadow-xs'
                : 'text-slate-600 dark:text-slate-400 hover:bg-slate-200/70 dark:hover:bg-slate-800 hover:text-slate-900 dark:hover:text-slate-100'
            }`}
            title={t('nav.settings')}
          >
            <Settings className="w-3.5 h-3.5 shrink-0" />
            <span className="whitespace-nowrap">{t('nav.settings')}</span>
          </button>
        </div>

        {/* Sync Center Modal */}
        {!isLocked && <SyncCenterModal isOpen={isSyncCenterOpen} onClose={() => setIsSyncCenterOpen(false)} />}

        {/* Background Job Manager Modal */}
        {!isLocked && <JobManagerModal isOpen={isJobManagerOpen} onClose={() => setIsJobManagerOpen(false)} />}

        {/* Changelog Modal */}
        {!isLocked && <ChangelogModal />}
      </div>
    </aside>
  );
};
