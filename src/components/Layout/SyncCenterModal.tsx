import React, { useState, useEffect } from 'react';
import { RefreshCw, CheckCircle2, X, Globe, Layers } from 'lucide-react';
import { tauriApi } from '../../services/tauri';
import { useSecurityStore } from '../../stores/useSecurityStore';
import { DiscoveredProfile, ImportJobItem } from '../../types';

interface SyncCenterModalProps {
  isOpen: boolean;
  onClose: () => void;
}

export const SyncCenterModal: React.FC<SyncCenterModalProps> = ({ isOpen, onClose }) => {
  const { isSyncing, triggerSyncAll } = useSecurityStore();
  const [sources, setSources] = useState<DiscoveredProfile[]>([]);
  const [jobs, setJobs] = useState<ImportJobItem[]>([]);

  const loadData = async () => {
    try {
      const [scanned, recentJobs] = await Promise.all([
        tauriApi.scanBrowsers(),
        tauriApi.getRecentImportJobs(10),
      ]);
      setSources(scanned);
      setJobs(recentJobs);
    } catch (e) {
      console.error('Failed to load sync center data:', e);
    }
  };

  useEffect(() => {
    if (isOpen) {
      loadData();
    }
  }, [isOpen]);

  const handleSyncAll = async () => {
    await triggerSyncAll();
    await loadData();
  };

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/60 backdrop-blur-sm animate-in fade-in duration-200">
      <div className="w-full max-w-2xl max-h-[85vh] bg-white dark:bg-slate-900 border border-slate-200 dark:border-slate-800 rounded-3xl shadow-2xl flex flex-col overflow-hidden">
        {/* Header */}
        <div className="p-6 border-b border-slate-100 dark:border-slate-800/80 flex items-center justify-between">
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-xl bg-indigo-50 dark:bg-indigo-950/60 border border-indigo-200 dark:border-indigo-800/60 flex items-center justify-center text-indigo-600 dark:text-indigo-400">
              <RefreshCw className={`w-5 h-5 ${isSyncing ? 'animate-spin' : ''}`} />
            </div>
            <div>
              <h2 className="text-lg font-bold text-slate-900 dark:text-slate-100">
                数据同步中心 (Sync Center)
              </h2>
              <p className="text-xs text-slate-500 dark:text-slate-400">
                查看浏览器数据源同步状态与历史增量导入任务
              </p>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <button
              onClick={handleSyncAll}
              disabled={isSyncing}
              className="px-3.5 py-2 rounded-xl bg-indigo-600 hover:bg-indigo-700 text-white text-xs font-semibold flex items-center gap-1.5 shadow-sm shadow-indigo-600/20 active:scale-95 transition disabled:opacity-50"
            >
              <RefreshCw className={`w-3.5 h-3.5 ${isSyncing ? 'animate-spin' : ''}`} />
              <span>{isSyncing ? '正在同步...' : '立即全部同步'}</span>
            </button>
            <button
              onClick={onClose}
              className="p-2 rounded-xl text-slate-400 hover:text-slate-600 dark:hover:text-slate-200 hover:bg-slate-100 dark:hover:bg-slate-800 transition"
            >
              <X className="w-5 h-5" />
            </button>
          </div>
        </div>

        {/* Content Body */}
        <div className="flex-1 overflow-y-auto p-6 space-y-6">
          {/* Section 1: Browser Sources */}
          <div>
            <h3 className="text-xs font-bold uppercase tracking-wider text-slate-400 dark:text-slate-500 mb-3 flex items-center gap-1.5">
              <Globe className="w-3.5 h-3.5" />
              <span>已检测浏览器数据源</span>
            </h3>

            <div className="border border-slate-200/80 dark:border-slate-800 rounded-2xl overflow-hidden">
              <table className="w-full text-left text-xs">
                <thead className="bg-slate-50/80 dark:bg-slate-800/50 text-slate-500 dark:text-slate-400 border-b border-slate-200/80 dark:border-slate-800">
                  <tr>
                    <th className="py-2.5 px-4 font-semibold">浏览器</th>
                    <th className="py-2.5 px-4 font-semibold">Profile</th>
                    <th className="py-2.5 px-4 font-semibold">历史文件大小</th>
                    <th className="py-2.5 px-4 font-semibold text-right">状态</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-slate-100 dark:divide-slate-800/60 text-slate-700 dark:text-slate-300">
                  {sources.length === 0 ? (
                    <tr>
                      <td colSpan={4} className="py-6 text-center text-slate-400">
                        未检测到主流浏览器历史文件
                      </td>
                    </tr>
                  ) : (
                    sources.map((s, i) => (
                      <tr key={i} className="hover:bg-slate-50/50 dark:hover:bg-slate-800/30 transition">
                        <td className="py-3 px-4 font-medium text-slate-900 dark:text-slate-100 capitalize">
                          {s.browser}
                        </td>
                        <td className="py-3 px-4 text-slate-500 dark:text-slate-400">
                          {s.profile_name || s.profile_id}
                        </td>
                        <td className="py-3 px-4 text-slate-500 font-mono">
                          {(s.history_size_bytes / 1024 / 1024).toFixed(2)} MB
                        </td>
                        <td className="py-3 px-4 text-right">
                          <span className="inline-flex items-center gap-1 text-[11px] px-2 py-0.5 rounded-full bg-emerald-50 text-emerald-600 dark:bg-emerald-950/60 dark:text-emerald-400 font-medium">
                            <CheckCircle2 className="w-3 h-3" />
                            已就绪
                          </span>
                        </td>
                      </tr>
                    ))
                  )}
                </tbody>
              </table>
            </div>
          </div>

          {/* Section 2: Recent Import Jobs */}
          <div>
            <h3 className="text-xs font-bold uppercase tracking-wider text-slate-400 dark:text-slate-500 mb-3 flex items-center gap-1.5">
              <Layers className="w-3.5 h-3.5" />
              <span>最近同步任务日志 (Import Jobs)</span>
            </h3>

            <div className="border border-slate-200/80 dark:border-slate-800 rounded-2xl overflow-hidden">
              <table className="w-full text-left text-xs">
                <thead className="bg-slate-50/80 dark:bg-slate-800/50 text-slate-500 dark:text-slate-400 border-b border-slate-200/80 dark:border-slate-800">
                  <tr>
                    <th className="py-2.5 px-4 font-semibold">任务时间</th>
                    <th className="py-2.5 px-4 font-semibold">目标源</th>
                    <th className="py-2.5 px-4 font-semibold text-right">读取数</th>
                    <th className="py-2.5 px-4 font-semibold text-right">新增导入</th>
                    <th className="py-2.5 px-4 font-semibold text-right">去重跳过</th>
                    <th className="py-2.5 px-4 font-semibold text-center">状态</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-slate-100 dark:divide-slate-800/60 text-slate-700 dark:text-slate-300">
                  {jobs.length === 0 ? (
                    <tr>
                      <td colSpan={6} className="py-6 text-center text-slate-400">
                        暂无导入任务记录
                      </td>
                    </tr>
                  ) : (
                    jobs.map((j) => {
                      const dateStr = new Date(j.started_at).toLocaleString();
                      const isSuccess = j.status === 'success';
                      return (
                        <tr key={j.id} className="hover:bg-slate-50/50 dark:hover:bg-slate-800/30 transition font-mono">
                          <td className="py-2.5 px-4 text-slate-500 dark:text-slate-400 font-sans">
                            {dateStr}
                          </td>
                          <td className="py-2.5 px-4 font-sans capitalize text-slate-900 dark:text-slate-100">
                            {j.browser ? `${j.browser} (${j.profile || 'Default'})` : '全部来源'}
                          </td>
                          <td className="py-2.5 px-4 text-right text-slate-500">{j.read_count}</td>
                          <td className="py-2.5 px-4 text-right font-semibold text-emerald-600 dark:text-emerald-400">
                            +{j.inserted_count}
                          </td>
                          <td className="py-2.5 px-4 text-right text-slate-400">{j.duplicate_count}</td>
                          <td className="py-2.5 px-4 text-center">
                            <span
                              className={`inline-block text-[11px] px-2 py-0.5 rounded-md font-sans ${
                                isSuccess
                                  ? 'bg-emerald-50 text-emerald-700 dark:bg-emerald-950/40 dark:text-emerald-400'
                                  : 'bg-rose-50 text-rose-700 dark:bg-rose-950/40 dark:text-rose-400'
                              }`}
                            >
                              {isSuccess ? '成功' : '失败'}
                            </span>
                          </td>
                        </tr>
                      );
                    })
                  )}
                </tbody>
              </table>
            </div>
          </div>
        </div>

        {/* Footer */}
        <div className="p-4 border-t border-slate-100 dark:border-slate-800/80 bg-slate-50/60 dark:bg-slate-900/60 flex items-center justify-between text-xs text-slate-500">
          <span>策略：应用启动首次解锁自动增量同步 + 48小时去重滑动窗口</span>
          <button
            onClick={onClose}
            className="px-4 py-1.5 rounded-lg bg-slate-200 dark:bg-slate-800 hover:bg-slate-300 dark:hover:bg-slate-700 text-slate-700 dark:text-slate-300 font-medium transition"
          >
            关闭
          </button>
        </div>
      </div>
    </div>
  );
};
