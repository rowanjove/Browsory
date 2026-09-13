import React, { useState } from 'react';
import { History, BarChart3, Database, Sparkles, Settings, Lock } from 'lucide-react';
import { useAppStore } from '../../stores/useAppStore';
import { useSecurityStore } from '../../stores/useSecurityStore';
import { NavTab } from '../../types';
import { AppLogo } from '../Common/AppLogo';
import { SyncCenterModal } from './SyncCenterModal';

export const Sidebar: React.FC = () => {
  const { currentTab, setCurrentTab, t } = useAppStore();
  const { pinEnabled, lock, isSyncing, lastSyncTime, lastSyncCount } = useSecurityStore();
  const [isSyncCenterOpen, setIsSyncCenterOpen] = useState(false);

  const navItems: { id: NavTab; label: string; icon: React.ReactNode }[] = [
    { id: 'history', label: t('nav.history'), icon: <History className="w-4 h-4" /> },
    { id: 'analytics', label: t('nav.analytics'), icon: <BarChart3 className="w-4 h-4" /> },
    { id: 'sources', label: t('nav.sources'), icon: <Database className="w-4 h-4" /> },
    { id: 'ai', label: t('nav.ai'), icon: <Sparkles className="w-4 h-4" /> },
  ];

  return (
    <aside className="w-48 bg-slate-50 dark:bg-slate-900 border-r border-slate-200 dark:border-slate-800 flex flex-col justify-between select-none shrink-0 h-full">
      <div className="flex flex-col">
        {/* App Title Header */}
        <div className="h-12 px-3 border-b border-slate-200 dark:border-slate-800 flex items-center justify-between">
          <div className="flex items-center gap-2 min-w-0">
            <AppLogo size={24} className="shrink-0 drop-shadow-sm" />
            <div className="flex flex-col min-w-0 leading-none">
              <span className="text-xs font-bold tracking-tight text-slate-800 dark:text-slate-100">
                Browsory
              </span>
              <span className="text-[10px] text-slate-400 dark:text-slate-500 font-medium tracking-wide mt-0.5">
                浏览足迹
              </span>
            </div>
          </div>
          <button
            type="button"
            onClick={() => setCurrentTab('settings')}
            className="text-[9px] px-1 py-0.5 rounded bg-slate-200/70 hover:bg-slate-300/70 dark:bg-slate-800 dark:hover:bg-slate-700 text-slate-500 hover:text-slate-700 dark:hover:text-slate-300 font-mono shrink-0 cursor-pointer transition"
            title="查看关于与版本更新说明"
          >
            v0.1.0
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
          onClick={() => setIsSyncCenterOpen(true)}
          className="w-full flex items-center justify-between px-2.5 py-1.5 rounded-lg bg-slate-100 dark:bg-slate-800/60 hover:bg-slate-200/70 dark:hover:bg-slate-800 transition text-[11px] text-slate-600 dark:text-slate-300"
          title="点击打开数据同步中心"
        >
          <div className="flex items-center gap-1.5 truncate">
            <span
              className={`w-1.5 h-1.5 rounded-full flex-shrink-0 ${
                isSyncing ? 'bg-amber-500 animate-ping' : 'bg-emerald-500'
              }`}
            />
            <span className="truncate">
              {isSyncing
                ? '正在同步...'
                : lastSyncTime
                ? `已同步 ${new Date(lastSyncTime).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}`
                : '同步就绪'}
            </span>
          </div>
          {lastSyncCount > 0 && !isSyncing && (
            <span className="text-[10px] font-mono font-semibold text-emerald-600 dark:text-emerald-400">
              +{lastSyncCount}
            </span>
          )}
        </button>

        {/* Bottom utility bar */}
        <div className="flex items-center justify-between px-1 text-[11px] text-slate-400">
          <div className="flex items-center gap-1.5" title="本地 SQLite WAL 归档库">
            <span className="font-mono text-[10px] tracking-wide text-slate-400 dark:text-slate-500">
              SQLite WAL
            </span>
          </div>

          <div className="flex items-center gap-1">
            {pinEnabled && (
              <button
                onClick={lock}
                className="p-1 rounded hover:bg-slate-200 dark:hover:bg-slate-800 text-slate-500 hover:text-slate-800 dark:hover:text-slate-200 transition-colors"
                title="立即锁定 (Cmd/Ctrl + L)"
              >
                <Lock className="w-3.5 h-3.5 text-slate-500 hover:text-amber-500 transition-colors" />
              </button>
            )}

            <button
              onClick={() => setCurrentTab('settings')}
              className={`flex items-center gap-1 px-2 py-1 rounded transition-colors cursor-pointer text-xs ${
                currentTab === 'settings'
                  ? 'bg-blue-600 text-white font-medium shadow-xs'
                  : 'text-slate-600 dark:text-slate-400 hover:bg-slate-200/70 dark:hover:bg-slate-800 hover:text-slate-900 dark:hover:text-slate-100'
              }`}
              title="系统设置"
            >
              <Settings className="w-3.5 h-3.5" />
              <span>设置</span>
            </button>
          </div>
        </div>

        {/* Sync Center Modal */}
        <SyncCenterModal isOpen={isSyncCenterOpen} onClose={() => setIsSyncCenterOpen(false)} />
      </div>
    </aside>
  );
};
