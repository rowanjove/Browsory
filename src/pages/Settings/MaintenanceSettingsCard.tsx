import React, { useState, useEffect } from 'react';
import {
  HardDrive,
  Database,
  CheckCircle2,
  AlertTriangle,
  Loader2,
  Trash2,
  Activity,
  FileCheck,
  RotateCcw,
  Download,
  ShieldCheck,
} from 'lucide-react';
import { save } from '@tauri-apps/plugin-dialog';
import { tauriApi } from '../../services/tauri';
import { useAppStore } from '../../stores/useAppStore';
import { BackupInfo, IntegrityReport } from '../../types';

export const MaintenanceSettingsCard: React.FC = () => {
  const { showToast } = useAppStore();
  const [backups, setBackups] = useState<BackupInfo[]>([]);
  const [isBackingUp, setIsBackingUp] = useState(false);
  const [isChecking, setIsChecking] = useState(false);
  const [isVacuuming, setIsVacuuming] = useState(false);
  const [integrityReport, setIntegrityReport] = useState<IntegrityReport | null>(null);

  const [restoreTarget, setRestoreTarget] = useState<BackupInfo | null>(null);
  const [isRestoring, setIsRestoring] = useState(false);
  const [isExportingId, setIsExportingId] = useState<number | null>(null);

  const loadBackups = async () => {
    try {
      const list = await tauriApi.listBackups();
      setBackups(list);
    } catch (e) {
      console.error('Failed to load backups:', e);
    }
  };

  useEffect(() => {
    loadBackups();
  }, []);

  const handleCreateBackup = async () => {
    setIsBackingUp(true);
    try {
      const b = await tauriApi.createDatabaseBackup('用户手动快照备份');
      showToast(`已成功创建数据库快照: ${b.file_name}`, 'success');
      await loadBackups();
    } catch (err: any) {
      showToast(typeof err === 'string' ? err : '备份失败', 'error');
    } finally {
      setIsBackingUp(false);
    }
  };

  const handleDeleteBackup = async (id: number) => {
    try {
      await tauriApi.deleteBackup(id);
      setBackups((prev) => prev.filter((b) => b.id !== id));
      showToast('备份文件已移除', 'info');
    } catch (err) {
      showToast('删除备份失败', 'error');
    }
  };

  const handleRestoreConfirm = async () => {
    if (!restoreTarget) return;
    setIsRestoring(true);
    try {
      const msg = await tauriApi.restoreDatabaseBackup(restoreTarget.id);
      showToast(msg, 'success');
      setRestoreTarget(null);
      await loadBackups();
    } catch (err: any) {
      showToast(typeof err === 'string' ? err : '恢复备份失败', 'error');
    } finally {
      setIsRestoring(false);
    }
  };

  const handleExportBackup = async (b: BackupInfo) => {
    setIsExportingId(b.id);
    try {
      const target = await save({
        defaultPath: b.file_name,
        filters: [{ name: 'SQLite Database', extensions: ['db'] }],
      });
      if (target) {
        await tauriApi.exportDatabaseBackup(b.id, target);
        showToast('备份文件已成功导出至指定路径', 'success');
      }
    } catch (err: any) {
      showToast(typeof err === 'string' ? err : '导出备份失败', 'error');
    } finally {
      setIsExportingId(null);
    }
  };

  const handleIntegrityCheck = async () => {
    setIsChecking(true);
    setIntegrityReport(null);
    try {
      const report = await tauriApi.checkDatabaseIntegrity();
      setIntegrityReport(report);
      if (report.is_healthy) {
        showToast('数据库健康状态良好', 'success');
      } else {
        showToast('数据库检查发现异常，请注意备份', 'warning');
      }
    } catch (err: any) {
      showToast('数据库检查失败', 'error');
    } finally {
      setIsChecking(false);
    }
  };

  const handleVacuum = async () => {
    setIsVacuuming(true);
    try {
      await tauriApi.vacuumDatabase();
      showToast('数据库碎片整理与压缩完成 (VACUUM 成功)', 'success');
    } catch (err) {
      showToast('压缩整理数据库失败', 'error');
    } finally {
      setIsVacuuming(false);
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
    <div className="bg-white dark:bg-slate-800 p-4 rounded-xl border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-4">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Database className="w-4 h-4 text-emerald-600 dark:text-emerald-400" />
          <span className="font-semibold text-slate-800 dark:text-slate-100">
            数据备份与恢复中心 (Backup & Restore Center)
          </span>
        </div>
        <span className="text-[10px] px-2 py-0.5 rounded-full bg-slate-100 dark:bg-slate-700 text-slate-500 font-mono">
          SQLite WAL Engine
        </span>
      </div>

      {/* Action Buttons Row */}
      <div className="flex flex-wrap gap-2.5">
        <button
          type="button"
          onClick={handleCreateBackup}
          disabled={isBackingUp}
          className="px-3 py-2 rounded-lg bg-emerald-600 hover:bg-emerald-700 text-white font-medium text-xs flex items-center gap-1.5 shadow-sm transition disabled:opacity-50 cursor-pointer"
        >
          {isBackingUp ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <HardDrive className="w-3.5 h-3.5" />}
          <span>{isBackingUp ? '正在备份...' : '立即备份数据库'}</span>
        </button>

        <button
          type="button"
          onClick={handleIntegrityCheck}
          disabled={isChecking}
          className="px-3 py-2 rounded-lg border border-slate-300 dark:border-slate-700 hover:bg-slate-100 dark:hover:bg-slate-700/60 text-slate-700 dark:text-slate-200 font-medium text-xs flex items-center gap-1.5 transition disabled:opacity-50 cursor-pointer"
        >
          {isChecking ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <FileCheck className="w-3.5 h-3.5 text-blue-500" />}
          <span>{isChecking ? '正在检查...' : '数据库完整性检查'}</span>
        </button>

        <button
          type="button"
          onClick={handleVacuum}
          disabled={isVacuuming}
          className="px-3 py-2 rounded-lg border border-slate-300 dark:border-slate-700 hover:bg-slate-100 dark:hover:bg-slate-700/60 text-slate-700 dark:text-slate-200 font-medium text-xs flex items-center gap-1.5 transition disabled:opacity-50 cursor-pointer"
        >
          {isVacuuming ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Activity className="w-3.5 h-3.5 text-amber-500" />}
          <span>{isVacuuming ? '正在整理...' : '整理与压缩 (VACUUM)'}</span>
        </button>
      </div>

      {/* Integrity Report Display */}
      {integrityReport && (
        <div className="p-3.5 rounded-lg bg-slate-50 dark:bg-slate-900/60 border border-slate-200 dark:border-slate-800 flex flex-col gap-2 text-xs">
          <div className="flex items-center gap-2 font-semibold">
            {integrityReport.is_healthy ? (
              <>
                <CheckCircle2 className="w-4 h-4 text-emerald-500" />
                <span className="text-emerald-700 dark:text-emerald-400">数据库结构完好无损</span>
              </>
            ) : (
              <>
                <AlertTriangle className="w-4 h-4 text-amber-500" />
                <span className="text-amber-700 dark:text-amber-400">检测到异常警告</span>
              </>
            )}
          </div>
          <div className="grid grid-cols-2 sm:grid-cols-4 gap-2 text-slate-600 dark:text-slate-400 font-mono text-[11px] pt-1">
            <div className="p-2 bg-white dark:bg-slate-800 rounded border border-slate-200/60 dark:border-slate-700/60">
              <div className="text-slate-400 text-[10px]">来源配置数</div>
              <div className="text-sm font-bold text-slate-800 dark:text-slate-100">{integrityReport.total_sources}</div>
            </div>
            <div className="p-2 bg-white dark:bg-slate-800 rounded border border-slate-200/60 dark:border-slate-700/60">
              <div className="text-slate-400 text-[10px]">独立 URL 数</div>
              <div className="text-sm font-bold text-slate-800 dark:text-slate-100">{integrityReport.total_urls}</div>
            </div>
            <div className="p-2 bg-white dark:bg-slate-800 rounded border border-slate-200/60 dark:border-slate-700/60">
              <div className="text-slate-400 text-[10px]">总访问记录数</div>
              <div className="text-sm font-bold text-slate-800 dark:text-slate-100">{integrityReport.total_visits}</div>
            </div>
            <div className="p-2 bg-white dark:bg-slate-800 rounded border border-slate-200/60 dark:border-slate-700/60">
              <div className="text-slate-400 text-[10px]">FTS 全文索引</div>
              <div className="text-sm font-bold text-emerald-600 dark:text-emerald-400">
                {integrityReport.fts_synced ? '已同步' : '需重建'}
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Backups List */}
      <div className="flex flex-col gap-2 pt-2 border-t border-slate-100 dark:border-slate-800">
        <div className="flex items-center justify-between">
          <span className="font-semibold text-slate-700 dark:text-slate-200">
            本地备份快照库 (自动保留最新 15 份)
          </span>
          <span className="text-[11px] text-slate-400">共 {backups.length} 份可用快照</span>
        </div>

        <div className="border border-slate-200 dark:border-slate-700 rounded-lg overflow-hidden">
          <table className="w-full text-left text-xs">
            <thead className="bg-slate-50 dark:bg-slate-900/60 text-slate-500 border-b border-slate-200 dark:border-slate-700">
              <tr>
                <th className="py-2 px-3">快照文件名</th>
                <th className="py-2 px-3">体积</th>
                <th className="py-2 px-3">备份时间</th>
                <th className="py-2 px-3 text-right">操作</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-100 dark:divide-slate-800">
              {backups.length === 0 ? (
                <tr>
                  <td colSpan={4} className="py-4 text-center text-slate-400">
                    暂无本地备份快照，点击上方按钮即可创建
                  </td>
                </tr>
              ) : (
                backups.map((b) => {
                  const dateStr = new Date(b.created_at).toLocaleString();
                  return (
                    <tr key={b.id} className="hover:bg-slate-50/50 dark:hover:bg-slate-800/50 font-mono">
                      <td className="py-2 px-3 text-slate-800 dark:text-slate-200">
                        <div className="flex flex-col">
                          <span className="font-medium">{b.file_name}</span>
                          {b.notes && (
                            <span className="text-[10px] text-slate-400 font-sans">
                              {b.notes}
                            </span>
                          )}
                        </div>
                      </td>
                      <td className="py-2 px-3 text-slate-500">
                        {formatBytes(b.file_size_bytes)}
                      </td>
                      <td className="py-2 px-3 text-slate-500 font-sans">
                        {dateStr}
                      </td>
                      <td className="py-2 px-3 text-right">
                        <div className="flex items-center justify-end gap-1 font-sans">
                          {/* Restore Button */}
                          <button
                            type="button"
                            onClick={() => setRestoreTarget(b)}
                            className="px-2 py-1 rounded bg-indigo-50 hover:bg-indigo-100 dark:bg-indigo-950/40 dark:hover:bg-indigo-900/60 text-indigo-700 dark:text-indigo-300 text-[11px] font-medium flex items-center gap-1 transition cursor-pointer"
                            title="从该备份还原系统数据"
                          >
                            <RotateCcw className="w-3 h-3" />
                            <span>恢复</span>
                          </button>

                          {/* Export Button */}
                          <button
                            type="button"
                            onClick={() => handleExportBackup(b)}
                            disabled={isExportingId === b.id}
                            className="px-2 py-1 rounded border border-slate-200 dark:border-slate-700 hover:bg-slate-100 dark:hover:bg-slate-700 text-slate-600 dark:text-slate-300 text-[11px] flex items-center gap-1 transition cursor-pointer disabled:opacity-50"
                            title="导出为独立文件"
                          >
                            {isExportingId === b.id ? (
                              <Loader2 className="w-3 h-3 animate-spin" />
                            ) : (
                              <Download className="w-3 h-3" />
                            )}
                            <span>导出</span>
                          </button>

                          {/* Delete Button */}
                          <button
                            type="button"
                            onClick={() => handleDeleteBackup(b.id)}
                            className="p-1 text-slate-400 hover:text-rose-500 transition cursor-pointer"
                            title="删除该备份"
                          >
                            <Trash2 className="w-3.5 h-3.5" />
                          </button>
                        </div>
                      </td>
                    </tr>
                  );
                })
              )}
            </tbody>
          </table>
        </div>
      </div>

      {/* Restore Safety Confirmation Modal */}
      {restoreTarget && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/60 backdrop-blur-sm p-4 animate-in fade-in duration-150">
          <div className="w-full max-w-md bg-white dark:bg-slate-800 rounded-2xl border border-slate-200 dark:border-slate-700 shadow-2xl overflow-hidden flex flex-col">
            <div className="p-5 flex flex-col gap-3">
              <div className="flex items-center gap-3 text-indigo-600 dark:text-indigo-400">
                <div className="w-10 h-10 rounded-xl bg-indigo-50 dark:bg-indigo-950/60 border border-indigo-200 dark:border-indigo-800/60 flex items-center justify-center shadow-inner">
                  <RotateCcw className="w-5 h-5" />
                </div>
                <div>
                  <h3 className="font-bold text-base text-slate-900 dark:text-slate-100">
                    确认恢复数据库备份？
                  </h3>
                  <p className="text-xs text-slate-500">
                    安全回滚与状态恢复
                  </p>
                </div>
              </div>

              <div className="p-3 bg-slate-50 dark:bg-slate-900/70 rounded-xl border border-slate-200/70 dark:border-slate-800 flex flex-col gap-1 text-xs">
                <div className="text-slate-500">目标快照:</div>
                <div className="font-mono text-slate-800 dark:text-slate-200 font-semibold break-all">
                  {restoreTarget.file_name}
                </div>
                <div className="text-[11px] text-slate-400 mt-1">
                  快照生成时间: {new Date(restoreTarget.created_at).toLocaleString()}
                </div>
              </div>

              {/* Safety Guarantee Alert */}
              <div className="p-3 bg-emerald-50/70 dark:bg-emerald-950/30 rounded-xl border border-emerald-200/80 dark:border-emerald-900/60 text-emerald-800 dark:text-emerald-300 text-xs flex items-start gap-2.5">
                <ShieldCheck className="w-4 h-4 text-emerald-600 dark:text-emerald-400 shrink-0 mt-0.5" />
                <div className="flex flex-col gap-0.5 leading-relaxed text-[11px]">
                  <span className="font-semibold">前置快照保护机制：</span>
                  <span>
                    执行恢复前，系统将自动为当前活动的数据库创建一份名为 <code className="font-mono bg-white/70 dark:bg-slate-900/70 px-1 py-0.5 rounded">archive_pre_restore_*.db</code> 的安全快照。即使恢复后想找回刚才的数据，也可以随时在列表中一键切回。
                  </span>
                </div>
              </div>
            </div>

            <div className="p-4 bg-slate-50 dark:bg-slate-850 border-t border-slate-200 dark:border-slate-700/80 flex items-center justify-end gap-2.5">
              <button
                type="button"
                onClick={() => setRestoreTarget(null)}
                disabled={isRestoring}
                className="px-3.5 py-1.5 rounded-lg border border-slate-300 dark:border-slate-700 hover:bg-slate-100 dark:hover:bg-slate-800 text-slate-700 dark:text-slate-300 text-xs font-medium transition cursor-pointer"
              >
                取消
              </button>
              <button
                type="button"
                onClick={handleRestoreConfirm}
                disabled={isRestoring}
                className="px-4 py-1.5 rounded-lg bg-indigo-600 hover:bg-indigo-700 text-white text-xs font-semibold shadow-md flex items-center gap-1.5 transition cursor-pointer disabled:opacity-50"
              >
                {isRestoring ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <RotateCcw className="w-3.5 h-3.5" />}
                <span>{isRestoring ? '正在还原数据库...' : '确认恢复'}</span>
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};
