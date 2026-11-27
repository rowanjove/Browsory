import React, { useCallback, useEffect, useRef, useState } from 'react';
import {
  CalendarClock,
  CheckCircle2,
  AlertTriangle,
  RefreshCw,
  Wrench,
  ChevronDown,
  ChevronUp,
  Clock,
  Database,
  ShieldAlert,
  Loader2,
} from 'lucide-react';
import { listen } from '@tauri-apps/api/event';
import { tauriApi } from '../../services/tauri';
import { bindDisposableListener } from '../../services/disposableListener';
import { createRequestSequence } from '../../services/requestSequence';
import { useAppStore } from '../../stores/useAppStore';
import { useHistoryStore } from '../../stores/useHistoryStore';
import { useSecurityStore } from '../../stores/useSecurityStore';
import { AutoSyncRunItem, AutoSyncStatus } from '../../types';

const PRESET_DAYS = [7, 30, 90] as const;

export const AutoSyncSettingsCard: React.FC = () => {
  const { language, showToast, setCurrentTab } = useAppStore();
  const isLocked = useSecurityStore((s) => s.isLocked);
  const ui = (zh: string, en: string) => (language === 'en-US' ? en : zh);

  const [status, setStatus] = useState<AutoSyncStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [syncingNow, setSyncingNow] = useState(false);
  const [repairing, setRepairing] = useState(false);

  const [customMode, setCustomMode] = useState(false);
  const [customDaysInput, setCustomDaysInput] = useState('30');
  const [showRuns, setShowRuns] = useState(false);
  const [runs, setRuns] = useState<AutoSyncRunItem[]>([]);
  const [loadingRuns, setLoadingRuns] = useState(false);

  const showRunsRef = useRef(showRuns);
  showRunsRef.current = showRuns;
  const mountedRef = useRef(true);
  const lastIntervalRef = useRef<number | null>(null);
  const statusSeqRef = useRef(createRequestSequence());
  const runsSeqRef = useRef(createRequestSequence());

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      statusSeqRef.current.next();
      runsSeqRef.current.next();
    };
  }, []);

  const syncIntervalInputFromStatus = useCallback((intervalDays: number) => {
    if (lastIntervalRef.current === intervalDays) return;
    lastIntervalRef.current = intervalDays;
    const isPreset = PRESET_DAYS.includes(intervalDays as (typeof PRESET_DAYS)[number]);
    setCustomMode(!isPreset);
    setCustomDaysInput(String(intervalDays));
  }, []);

  const loadStatus = useCallback(async () => {
    const seq = statusSeqRef.current.next();
    try {
      const data = await tauriApi.getAutoSyncStatus();
      if (!mountedRef.current || !statusSeqRef.current.isCurrent(seq)) return;
      setStatus(data);
      syncIntervalInputFromStatus(data.interval_days);
    } catch (err: any) {
      if (!mountedRef.current || !statusSeqRef.current.isCurrent(seq)) return;
      console.warn('Failed to load auto-sync status:', err);
    } finally {
      if (mountedRef.current && statusSeqRef.current.isCurrent(seq)) {
        setLoading(false);
      }
    }
  }, [syncIntervalInputFromStatus]);

  const loadRuns = useCallback(async () => {
    const seq = runsSeqRef.current.next();
    setLoadingRuns(true);
    try {
      const list = await tauriApi.listAutoSyncRuns(5);
      if (!mountedRef.current || !runsSeqRef.current.isCurrent(seq)) return;
      setRuns(list);
    } catch (err) {
      if (!mountedRef.current || !runsSeqRef.current.isCurrent(seq)) return;
      console.warn('Failed to load auto-sync runs:', err);
    } finally {
      if (mountedRef.current && runsSeqRef.current.isCurrent(seq)) {
        setLoadingRuns(false);
      }
    }
  }, []);

  useEffect(() => {
    if (isLocked) return;
    void loadStatus();
    return bindDisposableListener(() =>
      listen('browsory://auto-sync-updated', () => {
        if (useSecurityStore.getState().isLocked) return;
        void loadStatus();
        if (showRunsRef.current) void loadRuns();
      }),
    );
  }, [isLocked, loadStatus, loadRuns]);

  const handleToggleEnabled = async (nextEnabled: boolean) => {
    if (!status || saving) return;
    setSaving(true);
    try {
      const updated = await tauriApi.updateAutoSyncConfig(nextEnabled, status.interval_days);
      setStatus(updated);
      showToast(
        nextEnabled
          ? ui(
              `已开启自动同步（每 ${updated.interval_days} 天后台增量归档）`,
              `Auto-sync enabled (every ${updated.interval_days} days)`,
            )
          : ui('已关闭自动同步并清理系统计划任务', 'Auto-sync disabled and scheduled task removed'),
        'success',
      );
    } catch (err: any) {
      showToast(
        typeof err === 'string' ? err : err?.message || ui('更新自动同步开关失败', 'Failed to update auto-sync toggle'),
        'error',
      );
    } finally {
      setSaving(false);
    }
  };

  const handleSelectInterval = async (days: number) => {
    if (!status || saving) return;
    if (!Number.isInteger(days) || days < 1 || days > 365) {
      showToast(ui('同步周期必须为 1 ～ 365 天的整数', 'Interval must be an integer between 1 and 365 days'), 'warning');
      return;
    }
    setSaving(true);
    try {
      const updated = await tauriApi.updateAutoSyncConfig(status.enabled, days);
      setStatus(updated);
      lastIntervalRef.current = days;
      const isPreset = PRESET_DAYS.includes(days as (typeof PRESET_DAYS)[number]);
      setCustomMode(!isPreset);
      setCustomDaysInput(String(days));
      showToast(
        status.enabled
          ? ui(`同步周期已更新为每 ${days} 天，计划任务已同步更新`, `Sync interval updated to ${days} days`)
          : ui(`默认同步周期已设为 ${days} 天`, `Sync interval set to ${days} days`),
        'success',
      );
    } catch (err: any) {
      showToast(
        typeof err === 'string' ? err : err?.message || ui('更新同步周期失败', 'Failed to update sync interval'),
        'error',
      );
    } finally {
      setSaving(false);
    }
  };

  const handleRunNow = async () => {
    if (syncingNow) return;
    setSyncingNow(true);
    try {
      const run = await tauriApi.runAutoSyncNow();
      await loadStatus();
      if (showRuns) await loadRuns();
      if (run.total_inserted > 0) {
        void useHistoryStore.getState().fetchHistory(true);
      }
      if (run.status === 'failed') {
        showToast(
          run.error_summary || ui('同步执行失败，请检查数据源状态', 'Sync failed; inspect source status'),
          'error',
        );
      } else if (run.status === 'partial_success') {
        showToast(
          ui(
            `同步部分完成：新增 ${run.total_inserted} 条，${run.sources_failed} 个源失败`,
            `Partially synced: ${run.total_inserted} inserted, ${run.sources_failed} source(s) failed`,
          ),
          'warning',
        );
      } else {
        showToast(
          ui(
            `增量归档完成：新增 ${run.total_inserted} 条，去重 ${run.total_duplicate} 条`,
            `Incremental archive finished: ${run.total_inserted} inserted, ${run.total_duplicate} skipped`,
          ),
          'success',
        );
      }
    } catch (err: any) {
      showToast(
        typeof err === 'string' ? err : err?.message || ui('立即同步失败', 'Manual sync failed'),
        'error',
      );
    } finally {
      setSyncingNow(false);
    }
  };

  const handleRepairTask = async () => {
    if (repairing) return;
    setRepairing(true);
    try {
      const updated = await tauriApi.repairAutoSyncTask();
      setStatus(updated);
      showToast(ui('Windows 计划任务已修复并重新注册', 'Windows scheduled task repaired'), 'success');
    } catch (err: any) {
      showToast(
        typeof err === 'string' ? err : err?.message || ui('修复计划任务失败', 'Failed to repair scheduled task'),
        'error',
      );
    } finally {
      setRepairing(false);
    }
  };

  const handleToggleRuns = () => {
    const next = !showRuns;
    setShowRuns(next);
    if (next) {
      void loadRuns();
    }
  };

  const formatTs = (ts?: number | null) => {
    if (!ts) return '—';
    return new Date(ts).toLocaleString(language === 'en-US' ? 'en-US' : 'zh-CN', {
      month: '2-digit',
      day: '2-digit',
      hour: '2-digit',
      minute: '2-digit',
    });
  };

  const triggerLabel = (t: string) => {
    switch (t) {
      case 'scheduled':
        return ui('计划任务', 'Scheduled');
      case 'startup':
        return ui('启动补执行', 'Startup catch-up');
      case 'tray_due':
        return ui('托盘到期', 'Tray due');
      default:
        return ui('手动触发', 'Manual');
    }
  };

  const schedulerBadge = () => {
    if (!status) return { text: '...', tone: 'neutral' as const };
    switch (status.scheduler_status) {
      case 'registered':
        return { text: ui('任务计划 · 正常', 'Task Scheduler · OK'), tone: 'ok' as const };
      case 'disabled':
        return { text: ui('未启用调度', 'Disabled'), tone: 'neutral' as const };
      case 'missing':
        return { text: ui('计划任务缺失', 'Task missing'), tone: 'warn' as const };
      case 'outdated_path':
        return { text: ui('路径或周期待修复', 'Needs repair'), tone: 'warn' as const };
      case 'unsupported':
        return { text: ui('当前平台不支持', 'Unsupported OS'), tone: 'neutral' as const };
      default:
        return { text: ui('调度异常', 'Scheduler error'), tone: 'err' as const };
    }
  };

  const needsRepair =
    status?.enabled &&
    (status.scheduler_status === 'missing' ||
      status.scheduler_status === 'outdated_path' ||
      status.scheduler_status === 'error');

  const parsedCustom = Number(customDaysInput);
  const isCustomValid = Number.isInteger(parsedCustom) && parsedCustom >= 1 && parsedCustom <= 365;
  const badge = schedulerBadge();

  return (
    <div className="rounded-2xl border border-slate-200 dark:border-slate-800 bg-white dark:bg-slate-900 shadow-xs overflow-hidden">
      {/* Header with primary toggle */}
      <div className="p-5 flex items-center justify-between gap-4">
        <div className="flex items-center gap-3 min-w-0">
          <div className="p-2.5 rounded-xl bg-blue-500/10 text-blue-600 dark:text-blue-400 border border-blue-500/20 shrink-0">
            <CalendarClock className="w-5 h-5" />
          </div>
          <div className="min-w-0">
            <div className="flex items-center gap-2 flex-wrap">
              <h3 className="text-sm font-semibold text-slate-900 dark:text-slate-100">
                {ui('自动增量归档', 'Auto incremental archive')}
              </h3>
              {status && (
                <span
                  className={`px-2 py-0.5 text-[11px] font-medium rounded-full border ${
                    status.is_running || syncingNow
                      ? 'bg-blue-50 dark:bg-blue-950/60 text-blue-700 dark:text-blue-300 border-blue-200 dark:border-blue-800'
                      : status.enabled
                        ? needsRepair
                          ? 'bg-amber-50 dark:bg-amber-950/60 text-amber-700 dark:text-amber-300 border-amber-200 dark:border-amber-800'
                          : 'bg-emerald-50 dark:bg-emerald-950/60 text-emerald-700 dark:text-emerald-300 border-emerald-200 dark:border-emerald-800'
                        : 'bg-slate-100 dark:bg-slate-800 text-slate-500 dark:text-slate-400 border-slate-200 dark:border-slate-700'
                  }`}
                >
                  {status.is_running || syncingNow
                    ? ui('正在同步...', 'Syncing...')
                    : status.enabled
                      ? needsRepair
                        ? ui('计划任务待修复', 'Task needs repair')
                        : ui(`已启用 · 每 ${status.interval_days} 天`, `Enabled · Every ${status.interval_days}d`)
                      : ui('未启用', 'Disabled')}
                </span>
              )}
            </div>
            <p className="text-xs text-slate-500 dark:text-slate-400 mt-0.5">
              {ui(
                '通过 Windows 任务计划程序在后台低优先级唤醒并增量归档已绑定浏览器历史，无需主程序常驻。',
                'Uses Windows Task Scheduler at low priority to incrementally archive bound browsers in the background without keeping the app open.',
              )}
            </p>
          </div>
        </div>

        <label className="relative inline-flex items-center cursor-pointer shrink-0">
          <input
            type="checkbox"
            checked={Boolean(status?.enabled)}
            disabled={loading || saving}
            aria-label={ui('启用自动增量归档', 'Enable automatic incremental archive')}
            onChange={(e) => void handleToggleEnabled(e.target.checked)}
            className="sr-only peer"
          />
          <div className="w-9 h-5 bg-slate-200 peer-focus:outline-hidden rounded-full peer dark:bg-slate-700 peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-slate-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all dark:border-slate-600 peer-checked:bg-blue-600 peer-disabled:opacity-50"></div>
        </label>
      </div>

      {/* Body */}
      <div className="px-5 pb-5 pt-3 border-t border-slate-100 dark:border-slate-800 space-y-4">
        {/* Interval Selector */}
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
          <div>
            <div className="text-xs font-medium text-slate-700 dark:text-slate-300">
              {ui('自动同步周期', 'Sync interval')}
            </div>
            <div className="text-[11px] text-slate-400 mt-0.5">
              {ui('默认 30 天；支持 1 ～ 365 天自定义，错过开机后将自动补执行。', 'Defaults to 30 days (1–365 days). Missed runs catch up automatically.')}
            </div>
          </div>

          <div className="flex flex-wrap items-center gap-1.5">
            {PRESET_DAYS.map((days) => {
              const active = !customMode && status?.interval_days === days;
              return (
                <button
                  key={days}
                  type="button"
                  disabled={loading || saving}
                  onClick={() => {
                    void handleSelectInterval(days);
                  }}
                  className={`px-3 py-1.5 rounded-xl text-xs border transition cursor-pointer disabled:opacity-50 ${
                    active
                      ? 'border-blue-500 bg-blue-50 text-blue-700 dark:bg-blue-950/40 dark:text-blue-300 font-medium'
                      : 'border-slate-200 dark:border-slate-700 text-slate-600 dark:text-slate-300 hover:bg-slate-50 dark:hover:bg-slate-800'
                  }`}
                >
                  {ui(`${days} 天`, `${days}d`)}
                </button>
              );
            })}

            <button
              type="button"
              disabled={loading || saving}
              onClick={() => setCustomMode(true)}
              className={`px-3 py-1.5 rounded-xl text-xs border transition cursor-pointer disabled:opacity-50 ${
                customMode
                  ? 'border-blue-500 bg-blue-50 text-blue-700 dark:bg-blue-950/40 dark:text-blue-300 font-medium'
                  : 'border-slate-200 dark:border-slate-700 text-slate-600 dark:text-slate-300 hover:bg-slate-50 dark:hover:bg-slate-800'
              }`}
            >
              {ui('自定义', 'Custom')}
            </button>
          </div>
        </div>

        {customMode && (
          <div className="flex flex-wrap items-center gap-2 p-3 rounded-xl bg-slate-50 dark:bg-slate-800/50 border border-slate-200/80 dark:border-slate-800">
            <label className="text-xs text-slate-600 dark:text-slate-300" htmlFor="auto-sync-custom-days">
              {ui('自定义周期天数 (1–365):', 'Custom interval (1–365 days):')}
            </label>
            <input
              id="auto-sync-custom-days"
              type="number"
              min={1}
              max={365}
              step={1}
              value={customDaysInput}
              disabled={saving}
              onChange={(e) => setCustomDaysInput(e.target.value)}
              className="w-24 px-2.5 py-1.5 text-xs font-mono rounded-lg border border-slate-200 dark:border-slate-700 bg-white dark:bg-slate-900 text-slate-800 dark:text-slate-200 focus:outline-hidden focus:ring-1 focus:ring-blue-500"
            />
            <button
              type="button"
              disabled={saving || !isCustomValid}
              onClick={() => void handleSelectInterval(parsedCustom)}
              className="px-3 py-1.5 rounded-lg bg-blue-600 hover:bg-blue-700 text-white text-xs font-medium transition cursor-pointer disabled:opacity-50"
            >
              {ui('应用周期', 'Apply')}
            </button>
            {!isCustomValid && (
              <span className="text-[11px] text-rose-600 dark:text-rose-400">
                {ui('请输入 1～365 的整数', 'Enter an integer between 1 and 365')}
              </span>
            )}
          </div>
        )}

        {/* 4-cell Status Overview */}
        <div className="grid grid-cols-2 lg:grid-cols-4 gap-2.5 text-xs">
          <div className="p-3 rounded-xl bg-slate-50/80 dark:bg-slate-800/50 border border-slate-100 dark:border-slate-800">
            <div className="text-[11px] text-slate-400">{ui('调度状态', 'Scheduler')}</div>
            <div
              className={`mt-1 font-medium flex items-center gap-1.5 ${
                badge.tone === 'ok'
                  ? 'text-emerald-600 dark:text-emerald-400'
                  : badge.tone === 'warn'
                    ? 'text-amber-600 dark:text-amber-400'
                    : badge.tone === 'err'
                      ? 'text-rose-600 dark:text-rose-400'
                      : 'text-slate-600 dark:text-slate-300'
              }`}
            >
              {badge.tone === 'ok' ? (
                <CheckCircle2 className="w-3.5 h-3.5 shrink-0" />
              ) : badge.tone === 'warn' || badge.tone === 'err' ? (
                <AlertTriangle className="w-3.5 h-3.5 shrink-0" />
              ) : (
                <Clock className="w-3.5 h-3.5 shrink-0" />
              )}
              <span className="truncate">{badge.text}</span>
            </div>
          </div>

          <div className="p-3 rounded-xl bg-slate-50/80 dark:bg-slate-800/50 border border-slate-100 dark:border-slate-800">
            <div className="text-[11px] text-slate-400">{ui('已绑定数据源', 'Bound sources')}</div>
            <div className="mt-1 font-medium text-slate-800 dark:text-slate-200 flex items-center justify-between gap-2">
              <span className="flex items-center gap-1.5">
                <Database className="w-3.5 h-3.5 text-blue-500 shrink-0" />
                {status ? ui(`${status.bound_sources_count} 个已启用`, `${status.bound_sources_count} enabled`) : '...'}
              </span>
              {status && status.bound_sources_count === 0 && (
                <button
                  type="button"
                  onClick={() => setCurrentTab('sources')}
                  className="text-[11px] text-blue-600 dark:text-blue-400 hover:underline cursor-pointer"
                >
                  {ui('去绑定', 'Bind')}
                </button>
              )}
            </div>
          </div>

          <div className="p-3 rounded-xl bg-slate-50/80 dark:bg-slate-800/50 border border-slate-100 dark:border-slate-800">
            <div className="text-[11px] text-slate-400">{ui('最近执行', 'Last run')}</div>
            <div className="mt-1 font-medium text-slate-800 dark:text-slate-200 truncate">
              {status?.last_run ? (
                <span>
                  {formatTs(status.last_run.started_at)} ·{' '}
                  <span
                    className={
                      status.last_run.status === 'failed'
                        ? 'text-rose-600 dark:text-rose-400'
                        : status.last_run.status === 'partial_success'
                          ? 'text-amber-600 dark:text-amber-400'
                          : 'text-emerald-600 dark:text-emerald-400'
                    }
                  >
                    +{status.last_run.total_inserted}
                  </span>
                </span>
              ) : (
                ui('尚未执行', 'Never')
              )}
            </div>
          </div>

          <div className="p-3 rounded-xl bg-slate-50/80 dark:bg-slate-800/50 border border-slate-100 dark:border-slate-800">
            <div className="text-[11px] text-slate-400">{ui('下次计划', 'Next scheduled')}</div>
            <div className="mt-1 font-medium text-slate-800 dark:text-slate-200 truncate">
              {status?.enabled ? formatTs(status.next_run_at) : '—'}
            </div>
          </div>
        </div>

        {/* Contextual Warnings */}
        {status && !status.headless_capable && (
          <div
            role="status"
            className="p-3 rounded-xl bg-amber-50/80 dark:bg-amber-950/30 border border-amber-200 dark:border-amber-900 text-xs text-amber-800 dark:text-amber-200 flex items-start gap-2"
          >
            <ShieldAlert className="w-4 h-4 shrink-0 mt-0.5 text-amber-600 dark:text-amber-400" />
            <span>
              {ui(
                '当前保险库尚未绑定本机 DPAPI 自动解封或恢复密钥未确认，关闭主程序后无法后台自动解锁；请确认恢复密钥并重新解锁一次以启用无头后台归档。',
                'This vault has not enrolled Windows DPAPI auto-unseal or confirmed its recovery key yet. Background sync while closed requires DPAPI enrollment.',
              )}
            </span>
          </div>
        )}

        {status?.headless_notice && (
          <div
            role="status"
            className="p-3 rounded-xl bg-amber-50/80 dark:bg-amber-950/30 border border-amber-200 dark:border-amber-900 text-xs text-amber-800 dark:text-amber-200 flex items-start gap-2"
          >
            <AlertTriangle className="w-4 h-4 shrink-0 mt-0.5 text-amber-600 dark:text-amber-400" />
            <span>
              {ui('最近一次后台计划任务提示：', 'Last background task notice: ')}
              {status.headless_notice}
            </span>
          </div>
        )}

        {status?.scheduler_error && (
          <div
            role="alert"
            className="p-3 rounded-xl bg-rose-50/80 dark:bg-rose-950/30 border border-rose-200 dark:border-rose-900 text-xs text-rose-800 dark:text-rose-200 flex items-start gap-2"
          >
            <AlertTriangle className="w-4 h-4 shrink-0 mt-0.5 text-rose-600 dark:text-rose-400" />
            <span className="break-all">{status.scheduler_error}</span>
          </div>
        )}

        {/* Action Row */}
        <div className="flex flex-wrap items-center justify-between gap-2 pt-1">
          <div className="flex flex-wrap items-center gap-2">
            <button
              type="button"
              disabled={loading || syncingNow || Boolean(status?.is_running)}
              onClick={() => void handleRunNow()}
              className="px-3.5 py-2 rounded-xl bg-blue-600 hover:bg-blue-700 text-white text-xs font-medium flex items-center gap-1.5 shadow-xs transition cursor-pointer disabled:opacity-50"
            >
              {syncingNow || status?.is_running ? (
                <Loader2 className="w-3.5 h-3.5 animate-spin" />
              ) : (
                <RefreshCw className="w-3.5 h-3.5" />
              )}
              <span>{syncingNow || status?.is_running ? ui('正在归档...', 'Syncing...') : ui('立即同步', 'Sync now')}</span>
            </button>

            {needsRepair && (
              <button
                type="button"
                disabled={repairing}
                onClick={() => void handleRepairTask()}
                className="px-3 py-2 rounded-xl border border-amber-300 dark:border-amber-800 bg-amber-50 dark:bg-amber-950/40 hover:bg-amber-100 dark:hover:bg-amber-900/50 text-amber-800 dark:text-amber-300 text-xs font-medium flex items-center gap-1.5 transition cursor-pointer disabled:opacity-50"
              >
                {repairing ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Wrench className="w-3.5 h-3.5" />}
                <span>{ui('修复计划任务', 'Repair task')}</span>
              </button>
            )}

            <button
              type="button"
              onClick={handleToggleRuns}
              className="px-3 py-2 rounded-xl border border-slate-200 dark:border-slate-700 hover:bg-slate-50 dark:hover:bg-slate-800 text-slate-600 dark:text-slate-300 text-xs font-medium flex items-center gap-1.5 transition cursor-pointer"
            >
              <span>{ui('最近执行记录', 'Recent runs')}</span>
              {showRuns ? <ChevronUp className="w-3.5 h-3.5" /> : <ChevronDown className="w-3.5 h-3.5" />}
            </button>
          </div>

          {status?.is_portable && (
            <span className="text-[11px] text-slate-400">
              {ui('便携版移动目录后，启动时会自动修正计划任务路径', 'Portable mode auto-repairs task path on launch after moving folder')}
            </span>
          )}
        </div>

        {/* Recent Runs Expandable Table */}
        {showRuns && (
          <div className="border border-slate-200 dark:border-slate-800 rounded-xl overflow-hidden">
            {loadingRuns ? (
              <div className="p-4 text-center text-xs text-slate-400">
                {ui('正在加载执行记录...', 'Loading recent runs...')}
              </div>
            ) : runs.length === 0 ? (
              <div className="p-4 text-center text-xs text-slate-400">
                {ui('暂无自动或手动归档批次记录', 'No auto-sync runs recorded yet')}
              </div>
            ) : (
              <table className="w-full text-left text-xs">
                <thead className="bg-slate-50 dark:bg-slate-800/60 text-slate-500 border-b border-slate-200 dark:border-slate-800">
                  <tr>
                    <th className="py-2 px-3 font-medium">{ui('时间', 'Time')}</th>
                    <th className="py-2 px-3 font-medium">{ui('触发方式', 'Trigger')}</th>
                    <th className="py-2 px-3 font-medium text-right">{ui('新增 / 去重', 'Inserted / Dup')}</th>
                    <th className="py-2 px-3 font-medium text-right">{ui('耗时', 'Duration')}</th>
                    <th className="py-2 px-3 font-medium text-center">{ui('结果', 'Status')}</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-slate-100 dark:divide-slate-800">
                  {runs.map((run) => (
                    <tr key={run.id} title={run.error_summary || undefined} className="hover:bg-slate-50/50 dark:hover:bg-slate-800/30">
                      <td className="py-2 px-3 text-slate-600 dark:text-slate-300 font-mono">
                        {formatTs(run.started_at)}
                      </td>
                      <td className="py-2 px-3 text-slate-600 dark:text-slate-300">
                        {triggerLabel(run.trigger_type)}
                      </td>
                      <td className="py-2 px-3 text-right font-mono">
                        <span className="text-emerald-600 dark:text-emerald-400 font-medium">+{run.total_inserted}</span>
                        <span className="text-slate-400"> / {run.total_duplicate}</span>
                      </td>
                      <td className="py-2 px-3 text-right font-mono text-slate-500">
                        {run.duration_ms} ms
                      </td>
                      <td className="py-2 px-3 text-center">
                        <span
                          className={`inline-block px-2 py-0.5 rounded text-[11px] ${
                            run.status === 'success'
                              ? 'bg-emerald-50 text-emerald-700 dark:bg-emerald-950/40 dark:text-emerald-400'
                              : run.status === 'partial_success'
                                ? 'bg-amber-50 text-amber-700 dark:bg-amber-950/40 dark:text-amber-300'
                                : run.status === 'skipped'
                                  ? 'bg-slate-100 text-slate-600 dark:bg-slate-800 dark:text-slate-300'
                                  : 'bg-rose-50 text-rose-700 dark:bg-rose-950/40 dark:text-rose-400'
                          }`}
                        >
                          {run.status === 'success'
                            ? ui('成功', 'Success')
                            : run.status === 'partial_success'
                              ? ui('部分成功', 'Partial')
                              : run.status === 'skipped'
                                ? ui('已跳过', 'Skipped')
                                : ui('失败', 'Failed')}
                        </span>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        )}
      </div>
    </div>
  );
};
