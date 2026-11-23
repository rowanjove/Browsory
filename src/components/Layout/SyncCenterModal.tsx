import React, { useState, useEffect, useRef } from 'react';
import { RefreshCw, CheckCircle2, X, Globe, Layers } from 'lucide-react';
import { tauriApi } from '../../services/tauri';
import { createRequestSequence } from '../../services/requestSequence';
import { useSecurityStore } from '../../stores/useSecurityStore';
import { useAppStore } from '../../stores/useAppStore';
import { useDialogFocus } from '../../hooks/useDialogFocus';
import { DiscoveredProfile, ImportJobItem } from '../../types';

interface SyncCenterModalProps {
  isOpen: boolean;
  onClose: () => void;
}

export const SyncCenterModal: React.FC<SyncCenterModalProps> = ({ isOpen, onClose }) => {
  const { isSyncing, triggerSyncAll, lastSyncError } = useSecurityStore();
  const { language, t } = useAppStore();
  const ui = (zh: string, en: string) => (language === 'en-US' ? en : zh);
  const [sources, setSources] = useState<DiscoveredProfile[]>([]);
  const [jobs, setJobs] = useState<ImportJobItem[]>([]);
  const [loadingSources, setLoadingSources] = useState(false);
  const [loadingJobs, setLoadingJobs] = useState(false);
  const [sourcesError, setSourcesError] = useState<string | null>(null);
  const [jobsError, setJobsError] = useState<string | null>(null);
  const [syncError, setSyncError] = useState<string | null>(null);
  const sourcesSeqRef = useRef(createRequestSequence());
  const jobsSeqRef = useRef(createRequestSequence());

  const handleClose = () => {
    if (!isSyncing) onClose();
  };

  const dialogRef = useDialogFocus<HTMLDivElement>({
    isOpen,
    onClose: handleClose,
  });

  const loadSources = async () => {
    const seq = sourcesSeqRef.current.next();
    setLoadingSources(true);
    try {
      const scanned = await tauriApi.scanBrowsers();
      if (!sourcesSeqRef.current.isCurrent(seq)) return;
      setSources(scanned);
      setSourcesError(null);
    } catch (e) {
      if (!sourcesSeqRef.current.isCurrent(seq)) return;
      setSourcesError(e instanceof Error ? e.message : String(e));
    } finally {
      if (sourcesSeqRef.current.isCurrent(seq)) {
        setLoadingSources(false);
      }
    }
  };

  const loadJobs = async () => {
    const seq = jobsSeqRef.current.next();
    setLoadingJobs(true);
    try {
      const recentJobs = await tauriApi.getRecentImportJobs(10);
      if (!jobsSeqRef.current.isCurrent(seq)) return;
      setJobs(recentJobs);
      setJobsError(null);
    } catch (e) {
      if (!jobsSeqRef.current.isCurrent(seq)) return;
      setJobsError(e instanceof Error ? e.message : String(e));
    } finally {
      if (jobsSeqRef.current.isCurrent(seq)) {
        setLoadingJobs(false);
      }
    }
  };

  const loadData = async () => {
    await Promise.all([loadSources(), loadJobs()]);
  };

  useEffect(() => {
    if (!isOpen) {
      sourcesSeqRef.current.next();
      jobsSeqRef.current.next();
      return;
    }
    loadData();
    return () => {
      sourcesSeqRef.current.next();
      jobsSeqRef.current.next();
    };
  }, [isOpen]);

  const handleSyncAll = async () => {
    setSyncError(null);
    try {
      await triggerSyncAll();
    } catch (error) {
      setSyncError(error instanceof Error ? error.message : String(error));
    }
    await loadData();
  };

  if (!isOpen) return null;

  const visibleError = lastSyncError || syncError || sourcesError || jobsError;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/60 backdrop-blur-sm p-4 animate-in fade-in duration-200"
      onClick={handleClose}
      role="presentation"
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="sync-center-title"
        tabIndex={-1}
        onClick={(e) => e.stopPropagation()}
        className="w-full max-w-2xl max-h-[85vh] bg-white dark:bg-slate-900 border border-slate-200 dark:border-slate-800 rounded-3xl shadow-2xl flex flex-col overflow-hidden"
      >
        {/* Header */}
        <div className="p-6 border-b border-slate-100 dark:border-slate-800/80 flex items-center justify-between gap-4">
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-xl bg-blue-50 dark:bg-blue-950/60 border border-blue-200 dark:border-blue-800/60 flex items-center justify-center text-blue-600 dark:text-blue-400">
              <RefreshCw className={`w-5 h-5 ${isSyncing ? 'animate-spin' : ''}`} />
            </div>
            <div>
              <h2 id="sync-center-title" className="text-lg font-bold text-slate-900 dark:text-slate-100">
                {ui('数据同步中心', 'Sync Center')}
              </h2>
              <p className="text-xs text-slate-500 dark:text-slate-400">
                {ui('查看浏览器数据源同步状态与历史增量导入任务', 'Monitor browser source sync status and recent incremental import jobs')}
              </p>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={handleSyncAll}
              disabled={isSyncing}
              className="px-3.5 py-2 rounded-xl bg-blue-600 hover:bg-blue-700 text-white text-xs font-semibold flex items-center gap-1.5 shadow-sm transition disabled:opacity-50 cursor-pointer"
            >
              <RefreshCw className={`w-3.5 h-3.5 ${isSyncing ? 'animate-spin' : ''}`} />
              <span>{isSyncing ? ui('正在同步...', 'Syncing...') : ui('立即全部同步', 'Sync all now')}</span>
            </button>
            <button
              type="button"
              onClick={handleClose}
              disabled={isSyncing}
              aria-label={ui('关闭同步中心', 'Close Sync Center')}
              className="p-2 rounded-xl text-slate-400 hover:text-slate-600 dark:hover:text-slate-200 hover:bg-slate-100 dark:hover:bg-slate-800 transition disabled:opacity-40 cursor-pointer"
            >
              <X className="w-5 h-5" />
            </button>
          </div>
        </div>

        {visibleError && (
          <div role="alert" className="mx-5 mt-4 rounded border border-amber-300 bg-amber-50 p-3 text-xs text-amber-900 dark:border-amber-800 dark:bg-amber-950/40 dark:text-amber-200 whitespace-pre-wrap">
            {visibleError}
          </div>
        )}

        {/* Content Body */}
        <div className="flex-1 overflow-y-auto modal-scroll p-6 space-y-6">
          {/* Section 1: Browser Sources */}
          <div>
            <h3 className="text-xs font-bold uppercase tracking-wider text-slate-400 dark:text-slate-500 mb-3 flex items-center gap-1.5">
              <Globe className="w-3.5 h-3.5" />
              <span>{ui('已检测浏览器数据源', 'Detected browser sources')}</span>
            </h3>

            <div className="border border-slate-200/80 dark:border-slate-800 rounded-2xl overflow-hidden">
              <table className="w-full text-left text-xs">
                <thead className="bg-slate-50/80 dark:bg-slate-800/50 text-slate-500 dark:text-slate-400 border-b border-slate-200/80 dark:border-slate-800">
                  <tr>
                    <th className="py-2.5 px-4 font-semibold">{t('sources.browser')}</th>
                    <th className="py-2.5 px-4 font-semibold">{t('sources.profile')}</th>
                    <th className="py-2.5 px-4 font-semibold">{ui('历史文件大小', 'History size')}</th>
                    <th className="py-2.5 px-4 font-semibold text-right">{t('sources.status')}</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-slate-100 dark:divide-slate-800/60 text-slate-700 dark:text-slate-300">
                  {loadingSources && sources.length === 0 ? (
                    <tr>
                      <td colSpan={4} className="py-6 text-center text-slate-400">
                        {ui('正在检测浏览器数据源...', 'Scanning browser history sources...')}
                      </td>
                    </tr>
                  ) : sources.length === 0 ? (
                    <tr>
                      <td colSpan={4} className="py-6 text-center text-slate-400">
                        {ui('未检测到主流浏览器历史文件', 'No supported browser history files detected')}
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
                            {ui('已就绪', 'Ready')}
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
              <span>{ui('最近同步任务日志', 'Recent sync jobs')}</span>
            </h3>

            <div className="border border-slate-200/80 dark:border-slate-800 rounded-2xl overflow-hidden">
              <table className="w-full text-left text-xs">
                <thead className="bg-slate-50/80 dark:bg-slate-800/50 text-slate-500 dark:text-slate-400 border-b border-slate-200/80 dark:border-slate-800">
                  <tr>
                    <th className="py-2.5 px-4 font-semibold">{ui('任务时间', 'Started at')}</th>
                    <th className="py-2.5 px-4 font-semibold">{ui('目标源', 'Source')}</th>
                    <th className="py-2.5 px-4 font-semibold text-right">{ui('读取数', 'Read')}</th>
                    <th className="py-2.5 px-4 font-semibold text-right">{ui('新增导入', 'Inserted')}</th>
                    <th className="py-2.5 px-4 font-semibold text-right">{ui('去重跳过', 'Duplicate')}</th>
                    <th className="py-2.5 px-4 font-semibold text-center">{t('sources.status')}</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-slate-100 dark:divide-slate-800/60 text-slate-700 dark:text-slate-300">
                  {loadingJobs && jobs.length === 0 ? (
                    <tr>
                      <td colSpan={6} className="py-6 text-center text-slate-400">
                        {ui('正在读取最近同步记录...', 'Loading recent sync jobs...')}
                      </td>
                    </tr>
                  ) : jobs.length === 0 ? (
                    <tr>
                      <td colSpan={6} className="py-6 text-center text-slate-400">
                        {ui('暂无导入任务记录', 'No import job records yet')}
                      </td>
                    </tr>
                  ) : (
                    jobs.map((j) => {
                      const dateStr = new Date(j.started_at).toLocaleString();
                      const isSuccess = j.status === 'success';
                      return (
                        <tr title={j.error || undefined} key={j.id} className="hover:bg-slate-50/50 dark:hover:bg-slate-800/30 transition font-mono">
                          <td className="py-2.5 px-4 text-slate-500 dark:text-slate-400 font-sans">
                            {dateStr}
                          </td>
                          <td className="py-2.5 px-4 font-sans capitalize text-slate-900 dark:text-slate-100">
                            {j.browser ? `${j.browser} (${j.profile || 'Default'})` : ui('全部来源', 'All sources')}
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
                              {isSuccess ? ui('成功', 'Success') : ui('失败', 'Failed')}
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
          <span>
            {ui(
              '策略：解锁后自动增量同步 + 48 小时滑动窗口去重',
              'Policy: incremental sync after unlock + 48h sliding-window deduplication'
            )}
          </span>
          <button
            type="button"
            onClick={handleClose}
            disabled={isSyncing}
            className="px-4 py-1.5 rounded-lg bg-slate-200 dark:bg-slate-800 hover:bg-slate-300 dark:hover:bg-slate-700 text-slate-700 dark:text-slate-300 font-medium transition disabled:opacity-40 cursor-pointer"
          >
            {ui('关闭', 'Close')}
          </button>
        </div>
      </div>
    </div>
  );
};
