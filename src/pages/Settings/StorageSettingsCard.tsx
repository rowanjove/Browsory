import React, { useEffect, useState } from 'react';
import {
  HardDrive,
  Trash2,
  RefreshCw,
  Database,
  Archive,
  FileText,
  Clock,
} from 'lucide-react';
import { tauriApi } from '../../services/tauri';
import { StorageBreakdown } from '../../types';
import { useAppStore } from '../../stores/useAppStore';

export const StorageSettingsCard: React.FC = () => {
  const { showToast } = useAppStore();
  const [loading, setLoading] = useState(false);
  const [cleaning, setCleaning] = useState(false);
  const [storage, setStorage] = useState<StorageBreakdown | null>(null);

  const fetchStorage = async () => {
    setLoading(true);
    try {
      const data = await tauriApi.getStorageBreakdown();
      setStorage(data);
    } catch (err: any) {
      showToast('获取存储占用统计失败: ' + (err?.message || err), 'error');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchStorage();
  }, []);

  const handleCleanCache = async () => {
    setCleaning(true);
    try {
      const res = await tauriApi.cleanStorageCache();
      const freedFormatted = formatBytes(res.bytes_freed);
      showToast(
        `清理完成！已释放 ${freedFormatted} 磁盘空间 (删除了 ${res.temp_files_deleted} 个临时文件，${res.old_logs_deleted} 个旧日志)`,
        'success'
      );
      fetchStorage();
    } catch (err: any) {
      showToast('清理缓存失败: ' + (err?.message || err), 'error');
    } finally {
      setCleaning(false);
    }
  };

  const formatBytes = (bytes: number) => {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
  };

  const dbTotal = (storage?.database_bytes || 0) + (storage?.wal_bytes || 0);
  const backupsTotal = storage?.backups_bytes || 0;
  const logsTotal = storage?.logs_bytes || 0;
  const tempTotal = storage?.temp_bytes || 0;
  const total = storage?.total_bytes || 1;

  const dbPct = Math.round((dbTotal / total) * 100);
  const backupsPct = Math.round((backupsTotal / total) * 100);
  const logsPct = Math.round((logsTotal / total) * 100);
  const tempPct = Math.max(0, 100 - dbPct - backupsPct - logsPct);

  return (
    <div className="bg-white dark:bg-slate-900 rounded-2xl border border-slate-200 dark:border-slate-800 shadow-xs p-6">
      <div className="flex items-center justify-between mb-4">
        <div className="flex items-center gap-3">
          <div className="p-2 bg-emerald-50 dark:bg-emerald-900/30 text-emerald-600 dark:text-emerald-400 rounded-lg">
            <HardDrive className="w-5 h-5" />
          </div>
          <div>
            <h2 className="text-base font-semibold text-slate-800 dark:text-slate-100 flex items-center gap-2">
              存储占用管理
              <span className="text-xs px-2 py-0.5 rounded-full font-normal bg-emerald-100 dark:bg-emerald-950 text-emerald-700 dark:text-emerald-300">
                磁盘占用
              </span>
            </h2>
            <p className="text-xs text-slate-500 dark:text-slate-400">
              精确可视化主数据库、WAL 事务日志、历史备份、日志以及临时快照缓存的物理磁盘占用
            </p>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={fetchStorage}
            disabled={loading}
            className="flex items-center gap-1.5 px-3 py-1.5 text-xs text-slate-600 dark:text-slate-300 hover:bg-slate-100 dark:hover:bg-slate-700 rounded-lg border border-slate-200 dark:border-slate-600 transition-colors"
          >
            <RefreshCw className={`w-3.5 h-3.5 ${loading ? 'animate-spin' : ''}`} />
            刷新
          </button>
          <button
            onClick={handleCleanCache}
            disabled={cleaning || !storage}
            className="flex items-center gap-1.5 px-3.5 py-1.5 text-xs bg-slate-100 hover:bg-slate-200 dark:bg-slate-700 dark:hover:bg-slate-600 text-slate-700 dark:text-slate-200 rounded-lg font-medium transition-colors"
          >
            <Trash2 className="w-3.5 h-3.5 text-amber-500" />
            {cleaning ? '正在清理中...' : '清理缓存与过期日志'}
          </button>
        </div>
      </div>

      {storage ? (
        <div className="space-y-4">
          {/* Storage Total Banner */}
          <div className="flex items-baseline justify-between">
            <span className="text-xs text-slate-500 dark:text-slate-400">
              Browsory 数据本地总占用
            </span>
            <span className="text-lg font-bold text-slate-800 dark:text-slate-100 font-mono">
              {formatBytes(storage.total_bytes)}
            </span>
          </div>

          {/* Multi-segment Progress Bar */}
          <div className="w-full h-3 bg-slate-100 dark:bg-slate-900 rounded-full overflow-hidden flex">
            <div
              style={{ width: `${Math.max(dbPct, 2)}%` }}
              className="bg-blue-500 transition-all duration-300"
              title={`核心数据库: ${formatBytes(dbTotal)}`}
            />
            <div
              style={{ width: `${Math.max(backupsPct, 2)}%` }}
              className="bg-indigo-500 transition-all duration-300"
              title={`历史备份: ${formatBytes(backupsTotal)}`}
            />
            <div
              style={{ width: `${Math.max(logsPct, 1)}%` }}
              className="bg-amber-500 transition-all duration-300"
              title={`运行日志: ${formatBytes(logsTotal)}`}
            />
            <div
              style={{ width: `${Math.max(tempPct, 1)}%` }}
              className="bg-slate-400 transition-all duration-300"
              title={`临时缓存: ${formatBytes(tempTotal)}`}
            />
          </div>

          {/* Breakdown Items */}
          <div className="grid grid-cols-2 md:grid-cols-4 gap-3 pt-1">
            <div className="p-3 bg-slate-50 dark:bg-slate-900/50 rounded-lg border border-slate-200/70 dark:border-slate-700/70">
              <div className="flex items-center gap-1.5 text-xs text-slate-500 dark:text-slate-400 mb-1">
                <div className="w-2.5 h-2.5 rounded-full bg-blue-500" />
                <Database className="w-3.5 h-3.5 text-blue-500" />
                <span>SQLite 主库</span>
              </div>
              <div className="text-sm font-semibold text-slate-800 dark:text-slate-100 font-mono">
                {formatBytes(dbTotal)}
              </div>
              <div className="text-[10px] text-slate-400 mt-0.5">
                包含 WAL ({formatBytes(storage.wal_bytes)})
              </div>
            </div>

            <div className="p-3 bg-slate-50 dark:bg-slate-900/50 rounded-lg border border-slate-200/70 dark:border-slate-700/70">
              <div className="flex items-center gap-1.5 text-xs text-slate-500 dark:text-slate-400 mb-1">
                <div className="w-2.5 h-2.5 rounded-full bg-indigo-500" />
                <Archive className="w-3.5 h-3.5 text-indigo-500" />
                <span>历史安全备份</span>
              </div>
              <div className="text-sm font-semibold text-slate-800 dark:text-slate-100 font-mono">
                {formatBytes(backupsTotal)}
              </div>
              <div className="text-[10px] text-slate-400 mt-0.5">
                共 {storage.backups_count} 份独立快照
              </div>
            </div>

            <div className="p-3 bg-slate-50 dark:bg-slate-900/50 rounded-lg border border-slate-200/70 dark:border-slate-700/70">
              <div className="flex items-center gap-1.5 text-xs text-slate-500 dark:text-slate-400 mb-1">
                <div className="w-2.5 h-2.5 rounded-full bg-amber-500" />
                <FileText className="w-3.5 h-3.5 text-amber-500" />
                <span>滚动系统日志</span>
              </div>
              <div className="text-sm font-semibold text-slate-800 dark:text-slate-100 font-mono">
                {formatBytes(logsTotal)}
              </div>
              <div className="text-[10px] text-slate-400 mt-0.5">
                按日轮转保护
              </div>
            </div>

            <div className="p-3 bg-slate-50 dark:bg-slate-900/50 rounded-lg border border-slate-200/70 dark:border-slate-700/70">
              <div className="flex items-center gap-1.5 text-xs text-slate-500 dark:text-slate-400 mb-1">
                <div className="w-2.5 h-2.5 rounded-full bg-slate-400" />
                <Clock className="w-3.5 h-3.5 text-slate-400" />
                <span>临时提取快照</span>
              </div>
              <div className="text-sm font-semibold text-slate-800 dark:text-slate-100 font-mono">
                {formatBytes(tempTotal)}
              </div>
              <div className="text-[10px] text-slate-400 mt-0.5">
                同步分析临时缓存
              </div>
            </div>
          </div>
        </div>
      ) : (
        <div className="py-6 text-center text-xs text-slate-400">正在分析磁盘占用...</div>
      )}
    </div>
  );
};
