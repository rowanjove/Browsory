import React, { useEffect, useState } from 'react';
import {
  RefreshCw,
  Play,
  AlertCircle,
  CheckCircle2,
  Loader2,
  FolderOpen,
  Upload,
  Files,
  XCircle,
  FileJson,
} from 'lucide-react';
import { open } from '@tauri-apps/plugin-dialog';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { tauriApi } from '../../services/tauri';
import { BatchImportResult, DiscoveredProfile, TakeoutImportSummary } from '../../types';
import { useAppStore } from '../../stores/useAppStore';
import { useHistoryStore } from '../../stores/useHistoryStore';

export const SourcesPage: React.FC = () => {
  const { openRunningModal, showToast, t } = useAppStore();
  const { fetchHistory } = useHistoryStore();
  const [profiles, setProfiles] = useState<DiscoveredProfile[]>([]);
  const [isScanning, setIsScanning] = useState(false);
  const [syncingId, setSyncingId] = useState<string | null>(null);

  // Drag & drop and batch import states
  const [isDragging, setIsDragging] = useState(false);
  const [isImportingBatch, setIsImportingBatch] = useState(false);
  const [batchResult, setBatchResult] = useState<BatchImportResult | null>(null);

  // Google Takeout states
  const [isImportingTakeout, setIsImportingTakeout] = useState(false);
  const [takeoutResult, setTakeoutResult] = useState<TakeoutImportSummary | null>(null);

  const scan = async () => {
    setIsScanning(true);
    try {
      const list = await tauriApi.scanBrowsers();
      setProfiles(list);
    } catch (err: any) {
      showToast(err?.message || '扫描浏览器失败', 'error');
    } finally {
      setIsScanning(false);
    }
  };

  useEffect(() => {
    scan();
  }, []);

  // Listen to Tauri 2 native drag and drop events
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    try {
      const webview = getCurrentWebview();
      webview
        .onDragDropEvent((event) => {
          if (event.payload.type === 'over') {
            setIsDragging(true);
          } else if (event.payload.type === 'drop') {
            setIsDragging(false);
            const paths = event.payload.paths;
            if (paths && paths.length > 0) {
              handleProcessBatchFiles(paths);
            }
          } else {
            setIsDragging(false);
          }
        })
        .then((un) => {
          unlisten = un;
        })
        .catch(console.error);
    } catch (e) {
      console.warn('Webview drag drop listener initialization skipped:', e);
    }

    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  const handleProcessBatchFiles = async (filePaths: string[]) => {
    if (filePaths.length === 0) return;
    setIsImportingBatch(true);
    setBatchResult(null);
    showToast(`正在批量导入并清洗整理 ${filePaths.length} 个历史文件...`, 'info');

    try {
      const result = await tauriApi.importHistoryBatch(filePaths);
      setBatchResult(result);

      if (result.failed_files === 0) {
        showToast(
          `批量导入完成！成功 ${result.successful_files} 个文件，新增 ${result.total_inserted} 条，去重 ${result.total_duplicate} 条`,
          'success'
        );
      } else {
        showToast(
          `导入完成：成功 ${result.successful_files} 个，失败 ${result.failed_files} 个，新增 ${result.total_inserted} 条，去重 ${result.total_duplicate} 条`,
          result.successful_files > 0 ? 'warning' : 'error'
        );
      }

      fetchHistory(true);
      scan();
    } catch (err: any) {
      showToast(err?.message || '批量导入失败', 'error');
    } finally {
      setIsImportingBatch(false);
    }
  };

  const handleSelectFiles = async () => {
    try {
      const selected = await open({
        multiple: true,
        title: '选择浏览器 History 数据库或 Google Takeout JSON 归档（支持多选）',
        filters: [
          {
            name: '浏览器历史数据文件 (*.json, *.sqlite, *.*)',
            extensions: ['json', 'sqlite', 'sqlite3', 'db', '*'],
          },
          {
            name: 'Google Takeout (*.json)',
            extensions: ['json'],
          },
          {
            name: '所有文件 (*.*)',
            extensions: ['*'],
          },
        ],
      });
      if (!selected) return;

      const paths = Array.isArray(selected) ? selected : [selected];
      if (paths.length > 0) {
        await handleProcessBatchFiles(paths);
      }
    } catch (err: any) {
      showToast(err?.message || '选择文件失败', 'error');
    }
  };

  const handleSelectTakeoutFile = async () => {
    try {
      const selected = await open({
        multiple: false,
        filters: [
          { name: 'Google Takeout 历史文件 (*.json)', extensions: ['json'] },
          { name: '所有文件 (*.*)', extensions: ['*'] },
        ],
        title: '选择 Google Takeout 导出的 历史记录.json / Records.json / BrowserHistory.json',
      });
      if (!selected) return;
      const filePath = Array.isArray(selected) ? selected[0] : selected;
      if (!filePath) return;

      setIsImportingTakeout(true);
      setTakeoutResult(null);
      showToast('正在解析并流式导入 Google Takeout 历史...', 'info');

      const summary = await tauriApi.importTakeoutFile(filePath);
      setTakeoutResult(summary);
      showToast(
        `Google Takeout 导入成功！新增访问 ${summary.visits_inserted} 条，去重 ${summary.duplicates_skipped} 条 (耗时 ${summary.duration_ms}ms)`,
        'success'
      );
      fetchHistory(true);
      scan();
    } catch (err: any) {
      showToast(err?.message || '导入 Google Takeout 失败', 'error');
    } finally {
      setIsImportingTakeout(false);
    }
  };


  const handleSyncProfile = async (p: DiscoveredProfile) => {
    setSyncingId(`${p.browser}-${p.profile_id}`);
    try {
      // Check running
      const isRunning = await tauriApi.checkBrowserRunning(p.browser);
      if (isRunning) {
        openRunningModal(p.browser, p.source_id || undefined);
        setSyncingId(null);
        return;
      }

      // Sync specific source if available, otherwise fallback to syncAll
      if (p.source_id) {
        const res = await tauriApi.syncSource(p.source_id);
        showToast(
          t('sources.syncSuccess', {
            inserted: res.inserted_count,
            duplicate: res.duplicate_count,
          }),
          'success'
        );
      } else {
        const results = await tauriApi.syncAll();
        const match = results.find(
          (r) => r.browser.toLowerCase() === p.browser.toLowerCase() && r.profile === p.profile_id
        );
        if (match) {
          showToast(
            t('sources.syncSuccess', {
              inserted: match.inserted_count,
              duplicate: match.duplicate_count,
            }),
            'success'
          );
        } else {
          showToast('同步已完成', 'info');
        }
      }

      fetchHistory(true);
      scan();
    } catch (err: any) {
      showToast(err?.message || '同步失败', 'error');
    } finally {
      setSyncingId(null);
    }
  };

  const handleSyncAll = async () => {
    setIsScanning(true);
    try {
      const results = await tauriApi.syncAll();
      const inserted = results.reduce((acc, r) => acc + r.inserted_count, 0);
      const duplicate = results.reduce((acc, r) => acc + r.duplicate_count, 0);
      showToast(t('sources.syncSuccess', { inserted, duplicate }), 'success');
      fetchHistory(true);
      scan();
    } catch (err: any) {
      showToast(err?.message || '同步全部失败', 'error');
    } finally {
      setIsScanning(false);
    }
  };

  const formatBytes = (bytes: number) => {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
  };

  return (
    <div className="flex-1 flex flex-col h-full bg-slate-50 dark:bg-slate-900 overflow-y-auto p-6 text-xs select-none">
      {/* Page Header */}
      <div className="flex items-center justify-between pb-5 border-b border-slate-200 dark:border-slate-800">
        <div className="flex flex-col gap-1">
          <h1 className="text-base font-semibold text-slate-800 dark:text-slate-100">
            {t('sources.title')}
          </h1>
          <p className="text-slate-500">{t('sources.subtitle')}</p>
        </div>

        <div className="flex items-center gap-2">
          <button
            onClick={handleSelectTakeoutFile}
            disabled={isImportingTakeout || isImportingBatch}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded border border-indigo-300 dark:border-indigo-700 bg-indigo-50 dark:bg-indigo-950/50 hover:bg-indigo-100 dark:hover:bg-indigo-900/60 text-indigo-700 dark:text-indigo-300 transition font-medium cursor-pointer"
          >
            {isImportingTakeout ? (
              <Loader2 className="w-3.5 h-3.5 animate-spin" />
            ) : (
              <FileJson className="w-3.5 h-3.5" />
            )}
            <span>导入 Google Takeout (JSON)</span>
          </button>
          <button
            onClick={handleSelectFiles}
            disabled={isImportingBatch || isImportingTakeout}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded border border-blue-300 dark:border-blue-700 bg-blue-50 dark:bg-blue-950/50 hover:bg-blue-100 dark:hover:bg-blue-900/60 text-blue-700 dark:text-blue-300 transition font-medium cursor-pointer"
          >
            {isImportingBatch ? (
              <Loader2 className="w-3.5 h-3.5 animate-spin" />
            ) : (
              <Upload className="w-3.5 h-3.5" />
            )}
            <span>批量导入外部 History</span>
          </button>
          <button
            onClick={scan}
            disabled={isScanning}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded border border-slate-300 dark:border-slate-700 bg-white dark:bg-slate-800 hover:bg-slate-100 dark:hover:bg-slate-700 text-slate-700 dark:text-slate-200 transition cursor-pointer"
          >
            <RefreshCw className={`w-3.5 h-3.5 ${isScanning ? 'animate-spin' : ''}`} />
            <span>{t('sources.rescan')}</span>
          </button>
          <button
            onClick={handleSyncAll}
            disabled={isScanning}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded bg-blue-600 text-white hover:bg-blue-700 transition cursor-pointer"
          >
            <Play className="w-3.5 h-3.5" />
            <span>{t('sources.syncAll')}</span>
          </button>
        </div>
      </div>

      {/* Drag & Drop Zone */}
      <div
        onDragOver={(e) => {
          e.preventDefault();
          setIsDragging(true);
        }}
        onDragLeave={() => setIsDragging(false)}
        onDrop={(e) => {
          e.preventDefault();
          setIsDragging(false);
        }}
        onClick={handleSelectFiles}
        className={`mt-4 p-5 rounded-lg border-2 border-dashed transition-all cursor-pointer flex flex-col items-center justify-center gap-2 text-center select-none ${
          isDragging
            ? 'border-blue-500 bg-blue-50/80 dark:bg-blue-950/40 scale-[0.99] shadow-inner'
            : 'border-slate-300 dark:border-slate-700 hover:border-blue-400 dark:hover:border-blue-600 bg-white/60 dark:bg-slate-800/40 hover:bg-blue-50/20'
        }`}
      >
        <div className="w-10 h-10 rounded-full bg-blue-100 dark:bg-blue-950/80 text-blue-600 dark:text-blue-400 flex items-center justify-center">
          {isImportingBatch ? (
            <Loader2 className="w-5 h-5 animate-spin" />
          ) : (
            <Files className="w-5 h-5" />
          )}
        </div>
        <div className="flex flex-col gap-0.5">
          <span className="font-semibold text-slate-700 dark:text-slate-200 text-xs">
            {isDragging
              ? '松开鼠标即可立即批量解析并整理导入'
              : '拖入一个或多个浏览器 History 文件，或点击此处多选导入'}
          </span>
          <span className="text-[11px] text-slate-400">
            支持 Chromium 系 History、Firefox places.sqlite 数据库与 Google Takeout JSON 归档。入库时自动根据访问时刻与 URL 智能整理去重。
          </span>
        </div>
      </div>

      {/* Google Takeout Import Results Banner */}
      {takeoutResult && (
        <div className="mt-4 p-4 rounded-lg bg-white dark:bg-slate-800 border border-indigo-200 dark:border-indigo-800/60 shadow-sm flex flex-col gap-2.5">
          <div className="flex items-center justify-between pb-2 border-b border-slate-100 dark:border-slate-700">
            <div className="flex items-center gap-2">
              <CheckCircle2 className="w-4 h-4 text-indigo-500" />
              <span className="font-semibold text-slate-800 dark:text-slate-100">
                Google Takeout 归档导入完成
              </span>
            </div>
            <button
              onClick={() => setTakeoutResult(null)}
              className="text-slate-400 hover:text-slate-600 dark:hover:text-slate-200 text-[11px] cursor-pointer"
            >
              关闭报告
            </button>
          </div>

          <div className="grid grid-cols-4 gap-3 text-center">
            <div className="p-2 rounded bg-slate-50 dark:bg-slate-900">
              <div className="text-slate-400 text-[10px]">解析条目</div>
              <div className="font-semibold text-slate-800 dark:text-slate-200 text-sm mt-0.5">
                {takeoutResult.total_records_parsed.toLocaleString()}
              </div>
            </div>
            <div className="p-2 rounded bg-indigo-50/60 dark:bg-indigo-950/30">
              <div className="text-indigo-600 dark:text-indigo-400 text-[10px]">新增访问记录</div>
              <div className="font-semibold text-indigo-700 dark:text-indigo-300 text-sm mt-0.5">
                +{takeoutResult.visits_inserted.toLocaleString()}
              </div>
            </div>
            <div className="p-2 rounded bg-amber-50/60 dark:bg-amber-950/30">
              <div className="text-amber-600 dark:text-amber-400 text-[10px]">智能去重跳过</div>
              <div className="font-semibold text-amber-700 dark:text-amber-300 text-sm mt-0.5">
                {takeoutResult.duplicates_skipped.toLocaleString()}
              </div>
            </div>
            <div className="p-2 rounded bg-slate-50 dark:bg-slate-900">
              <div className="text-slate-400 text-[10px]">导入耗时</div>
              <div className="font-semibold text-slate-700 dark:text-slate-300 text-sm mt-0.5">
                {(takeoutResult.duration_ms / 1000).toFixed(2)}s
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Batch Import Results Banner */}
      {batchResult && (
        <div className="mt-4 p-4 rounded-lg bg-white dark:bg-slate-800 border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-2.5">
          <div className="flex items-center justify-between pb-2 border-b border-slate-100 dark:border-slate-700">
            <div className="flex items-center gap-2">
              <CheckCircle2 className="w-4 h-4 text-emerald-500" />
              <span className="font-semibold text-slate-800 dark:text-slate-100">
                批量导入整理完成
              </span>
            </div>
            <button
              onClick={() => setBatchResult(null)}
              className="text-slate-400 hover:text-slate-600 dark:hover:text-slate-200 text-[11px]"
            >
              关闭报告
            </button>
          </div>

          <div className="grid grid-cols-4 gap-3 text-center">
            <div className="p-2 rounded bg-slate-50 dark:bg-slate-900">
              <div className="text-slate-400 text-[10px]">处理文件</div>
              <div className="font-semibold text-slate-800 dark:text-slate-200 text-sm mt-0.5">
                {batchResult.successful_files} / {batchResult.total_files}
              </div>
            </div>
            <div className="p-2 rounded bg-emerald-50/60 dark:bg-emerald-950/30">
              <div className="text-emerald-600 dark:text-emerald-400 text-[10px]">新增记录</div>
              <div className="font-semibold text-emerald-700 dark:text-emerald-300 text-sm mt-0.5">
                +{batchResult.total_inserted.toLocaleString()}
              </div>
            </div>
            <div className="p-2 rounded bg-amber-50/60 dark:bg-amber-950/30">
              <div className="text-amber-600 dark:text-amber-400 text-[10px]">智能去重</div>
              <div className="font-semibold text-amber-700 dark:text-amber-300 text-sm mt-0.5">
                {batchResult.total_duplicate.toLocaleString()}
              </div>
            </div>
            <div className="p-2 rounded bg-slate-50 dark:bg-slate-900">
              <div className="text-slate-400 text-[10px]">异常文件</div>
              <div className={`font-semibold text-sm mt-0.5 ${batchResult.failed_files > 0 ? 'text-rose-600' : 'text-slate-500'}`}>
                {batchResult.failed_files}
              </div>
            </div>
          </div>

          {/* Details list if there are failed files */}
          {batchResult.results.some((r) => !r.success) && (
            <div className="mt-1 p-2.5 rounded bg-rose-50/60 dark:bg-rose-950/30 border border-rose-200 dark:border-rose-900 text-[11px] flex flex-col gap-1">
              <span className="font-medium text-rose-700 dark:text-rose-300 flex items-center gap-1">
                <XCircle className="w-3.5 h-3.5" />
                部分文件处理异常：
              </span>
              {batchResult.results
                .filter((r) => !r.success)
                .map((r, idx) => (
                  <div key={idx} className="text-rose-600 dark:text-rose-400 pl-4 list-disc">
                    • <strong>{r.file_name}</strong>: {r.error}
                  </div>
                ))}
            </div>
          )}
        </div>
      )}

      {/* Discovered Profiles Table Header */}
      <div className="mt-5 flex items-center justify-between">
        <span className="font-semibold text-slate-800 dark:text-slate-100 text-xs">
          自动发现的浏览器 Profile 数据源
        </span>
      </div>

      {/* Discovered Profiles Table */}
      <div className="mt-2 bg-white dark:bg-slate-800 border border-slate-200 dark:border-slate-700/80 rounded overflow-hidden shadow-sm">
        <table className="w-full text-left border-collapse">
          <thead>
            <tr className="border-b border-slate-200 dark:border-slate-700 bg-slate-100 dark:bg-slate-800/80 text-slate-500 font-medium h-9">
              <th className="px-4">{t('sources.browser')}</th>
              <th className="px-4">{t('sources.profile')}</th>
              <th className="px-4">{t('sources.historyFile')}</th>
              <th className="px-4">大小</th>
              <th className="px-4">{t('sources.status')}</th>
              <th className="px-4 text-right">{t('sources.action')}</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-slate-100 dark:divide-slate-700/50">
            {profiles.length === 0 ? (
              <tr>
                <td colSpan={6} className="text-center py-8 text-slate-400">
                  {isScanning ? '正在扫描本机浏览器...' : '未发现支持的浏览器数据库'}
                </td>
              </tr>
            ) : (
              profiles.map((p) => {
                const isSyncingCurrent = syncingId === `${p.browser}-${p.profile_id}`;
                return (
                  <tr
                    key={`${p.browser}-${p.profile_id}`}
                    className="hover:bg-slate-50 dark:hover:bg-slate-750 transition-colors h-11"
                  >
                    <td className="px-4 font-medium text-slate-800 dark:text-slate-200 capitalize">
                      {p.browser}
                    </td>
                    <td className="px-4 text-slate-600 dark:text-slate-300">
                      {p.profile_name}
                      {p.profile_name !== p.profile_id && (
                        <span className="ml-1 text-slate-400 font-mono text-[11px]">
                          ({p.profile_id})
                        </span>
                      )}
                    </td>
                    <td className="px-4 font-mono text-[11px] text-slate-400 dark:text-slate-500 max-w-[260px] truncate" title={p.history_path}>
                      {p.history_path}
                    </td>
                    <td className="px-4 font-mono text-slate-500">
                      {formatBytes(p.history_size_bytes)}
                    </td>
                    <td className="px-4">
                      {p.is_running ? (
                        <span className="inline-flex items-center gap-1 text-amber-600 dark:text-amber-400 bg-amber-50 dark:bg-amber-950/50 px-2 py-0.5 rounded text-[11px]">
                          <AlertCircle className="w-3 h-3" />
                          {t('sources.running')}
                        </span>
                      ) : (
                        <span className="inline-flex items-center gap-1 text-emerald-600 dark:text-emerald-400 bg-emerald-50 dark:bg-emerald-950/50 px-2 py-0.5 rounded text-[11px]">
                          <CheckCircle2 className="w-3 h-3" />
                          {t('sources.closed')}
                        </span>
                      )}
                    </td>
                    <td className="px-4 text-right flex items-center justify-end gap-1.5 pt-2.5">
                      <button
                        onClick={() => tauriApi.openPathInFolder(p.history_path)}
                        className="p-1 rounded hover:bg-slate-200 dark:hover:bg-slate-700 text-slate-500 hover:text-slate-800 dark:hover:text-slate-200 transition"
                        title="在资源管理器中定位"
                      >
                        <FolderOpen className="w-3.5 h-3.5" />
                      </button>
                      <button
                        onClick={() => handleSyncProfile(p)}
                        disabled={isSyncingCurrent}
                        className="px-2.5 py-1 rounded bg-slate-100 dark:bg-slate-700 hover:bg-blue-600 hover:text-white dark:hover:bg-blue-600 transition text-slate-700 dark:text-slate-200 font-medium inline-flex items-center gap-1"
                      >
                        {isSyncingCurrent ? (
                          <Loader2 className="w-3 h-3 animate-spin" />
                        ) : (
                          <Play className="w-3 h-3" />
                        )}
                        <span>{t('sources.sync')}</span>
                      </button>
                    </td>
                  </tr>
                );
              })
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
};
