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
  X,
} from 'lucide-react';
import { open, save } from '@tauri-apps/plugin-dialog';
import { tauriApi } from '../../services/tauri';
import { useAppStore } from '../../stores/useAppStore';
import { useSecurityStore } from '../../stores/useSecurityStore';
import { useDialogFocus } from '../../hooks/useDialogFocus';
import { BackupInfo, IntegrityReport } from '../../types';
import { LegacyArchiveRetirementCard } from './LegacyArchiveRetirementCard';
import { useSensitiveAction } from '../../hooks/useSensitiveAction';

export const MaintenanceSettingsCard: React.FC = () => {
  const { showToast, language, t } = useAppStore();
  const hasDpapiWrap = useSecurityStore((s) => s.hasDpapiWrap);
  const captureAction = useSensitiveAction();
  const ui = (zh: string, en: string) => (language === 'en-US' ? en : zh);

  const [backups, setBackups] = useState<BackupInfo[]>([]);
  const [isBackingUp, setIsBackingUp] = useState(false);
  const [isChecking, setIsChecking] = useState(false);
  const [isVacuuming, setIsVacuuming] = useState(false);
  const [integrityReport, setIntegrityReport] = useState<IntegrityReport | null>(null);
  const [confirmDeleteBackupId, setConfirmDeleteBackupId] = useState<number | null>(null);

  const [restoreTarget, setRestoreTarget] = useState<BackupInfo | null>(null);
  const [restoreFile, setRestoreFile] = useState<string | null>(null);
  const [credentialMode, setCredentialMode] = useState<'current' | 'password' | 'recovery'>('current');
  const [credential, setCredential] = useState('');
  const [isRestoring, setIsRestoring] = useState(false);
  const [isExportingId, setIsExportingId] = useState<number | null>(null);
  const [residue, setResidue] = useState<{ legacy_backup_bytes: number | null; blocked: boolean } | null>(null);
  const [retirementPassword, setRetirementPassword] = useState('');
  const [retirementConfirmation, setRetirementConfirmation] = useState('');
  const [isRetiring, setIsRetiring] = useState(false);
  const [residueError, setResidueError] = useState('');

  const handleCloseRestoreModal = () => {
    if (isRestoring) return;
    setRestoreTarget(null);
    setRestoreFile(null);
    setCredential('');
  };

  const restoreDialogRef = useDialogFocus<HTMLDivElement>({
    isOpen: Boolean(restoreTarget || restoreFile),
    onClose: handleCloseRestoreModal,
  });

  const loadResidue = async () => {
    try {
      setResidue(await tauriApi.getPlaintextResidue());
    } catch {
      setResidueError(ui('旧明文副本状态无法读取，请处理错误后重试。', 'Unable to inspect legacy plaintext backup state; please retry.'));
    }
  };

  const handleRetirement = async () => {
    if (isRetiring || (!hasDpapiWrap && !retirementPassword) || retirementConfirmation !== '移除旧明文副本') return;
    setIsRetiring(true);
    setResidueError('');
    try {
      const message = await tauriApi.retireLegacyPlaintextBackup(hasDpapiWrap ? '' : retirementPassword, retirementConfirmation);
      showToast(message, 'success');
    } catch (error) {
      setResidueError(
        typeof error === 'string'
          ? error
          : ui('归档未完成，请检查旧副本与加密归档状态。', 'Retirement incomplete; check the legacy backup and encrypted archive.')
      );
    } finally {
      setRetirementPassword('');
      setRetirementConfirmation('');
      setIsRetiring(false);
      await loadResidue();
    }
  };

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
    void loadResidue();
  }, []);

  const handleCreateBackup = async () => {
    setIsBackingUp(true);
    try {
      const b = await tauriApi.createDatabaseBackup(ui('用户手动快照备份', 'Manual backup snapshot'));
      showToast(ui(`已成功创建数据库快照: ${b.file_name}`, `Created database backup: ${b.file_name}`), 'success');
      await loadBackups();
    } catch (err: any) {
      showToast(typeof err === 'string' ? err : ui('备份失败', 'Backup failed'), 'error');
    } finally {
      setIsBackingUp(false);
    }
  };

  const handleDeleteBackup = async (id: number) => {
    setConfirmDeleteBackupId(null);
    try {
      await tauriApi.deleteBackup(id);
      setBackups((prev) => prev.filter((b) => b.id !== id));
      showToast(ui('备份文件已移除', 'Backup removed'), 'info');
    } catch {
      showToast(ui('删除备份失败', 'Failed to delete backup'), 'error');
    }
  };

  const handleRestoreConfirm = async () => {
    if ((!restoreTarget && !restoreFile) || isRestoring) return;
    if (credentialMode !== 'current' && !credential) return;
    setIsRestoring(true);
    try {
      const password = credentialMode === 'password' ? credential : undefined;
      const recovery = credentialMode === 'recovery' ? credential : undefined;
      const msg = await useSecurityStore.getState().restoreVaultBackup(restoreFile ?? restoreTarget!.id, password, recovery);
      showToast(msg, 'success');
      setRestoreTarget(null);
      setRestoreFile(null);
    } catch (err: any) {
      showToast(typeof err === 'string' ? err : ui('恢复备份失败', 'Failed to restore backup'), 'error');
    } finally {
      setCredential('');
      setIsRestoring(false);
    }
  };

  const handleImportBackup = async () => {
    try {
      const assertActionCurrent = captureAction();
      const selected = await open({
        multiple: false,
        filters: [{ name: 'Browsory Encrypted Backup', extensions: ['browsory-backup'] }],
      });
      assertActionCurrent();
      if (typeof selected === 'string') {
        setRestoreTarget(null);
        setRestoreFile(selected);
        setCredentialMode(hasDpapiWrap ? 'current' : 'password');
        setCredential('');
      }
    } catch (err: any) {
      showToast(typeof err === 'string' ? err : ui('保险库会话已变化，未导入备份', 'Vault session changed; backup was not imported'), 'error');
    }
  };

  const handleExportBackup = async (b: BackupInfo) => {
    setIsExportingId(b.id);
    try {
      const assertActionCurrent = captureAction();
      const target = await save({
        defaultPath: b.file_name,
        filters: [
          { name: 'Browsory Encrypted Backup', extensions: ['browsory-backup'] },
          { name: 'All Files', extensions: ['*'] },
        ],
      });
      assertActionCurrent();
      if (target) {
        await tauriApi.exportDatabaseBackup(b.id, target);
        showToast(ui('备份文件已成功导出至指定路径', 'Backup exported'), 'success');
      }
    } catch (err: any) {
      showToast(typeof err === 'string' ? err : ui('导出备份失败', 'Failed to export backup'), 'error');
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
        showToast(ui('数据库健康状态良好', 'Database integrity check passed'), 'success');
      } else {
        showToast(ui('数据库检查发现异常，请注意备份', 'Integrity issues detected; back up your vault'), 'warning');
      }
    } catch {
      showToast(ui('数据库检查失败', 'Integrity check failed'), 'error');
    } finally {
      setIsChecking(false);
    }
  };

  const handleVacuum = async () => {
    setIsVacuuming(true);
    try {
      await tauriApi.vacuumDatabase();
      showToast(ui('数据库碎片整理与压缩完成 (VACUUM 成功)', 'Database compacted (VACUUM succeeded)'), 'success');
    } catch {
      showToast(ui('压缩整理数据库失败', 'Database compaction failed'), 'error');
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
            {ui('数据备份与恢复中心 (Backup & Restore Center)', 'Backup & Restore Center')}
          </span>
        </div>
        <span className="text-[11px] px-2 py-0.5 rounded-full bg-slate-100 dark:bg-slate-700 text-slate-500 font-mono">
          SQLite WAL Engine
        </span>
      </div>

      <div className="rounded-xl border border-amber-200 dark:border-amber-900 bg-amber-50/70 dark:bg-amber-950/20 p-3 text-xs space-y-2">
        <div className="font-semibold text-amber-900 dark:text-amber-200">
          {ui('迁移保留的旧明文副本', 'Migration-retained legacy plaintext backup')}
        </div>
        <p className="text-slate-600 dark:text-slate-400">
          {ui(
            '同步使用有界内存快照或加密暂存。旧版迁移留下的 archive.db.plaintext.bak 仍可能被读取；此处仅处理这一个文件，不扫描其他目录。',
            'Normal sync uses bounded in-memory snapshots. A legacy archive.db.plaintext.bak from migration may still exist; this section retires only that single file.'
          )}
        </p>
        {residue?.legacy_backup_bytes != null ? (
          <>
            <p>
              {ui(
                `检测到旧副本：${formatBytes(residue.legacy_backup_bytes)}。将先逐项校验并保存加密数据库与信封，再移除该明文文件。`,
                `Detected legacy backup (${formatBytes(residue.legacy_backup_bytes)}). Its encrypted database and envelope copy are verified and saved before removing the plaintext file.`
              )}
            </p>
            <p className="text-amber-800 dark:text-amber-300">
              {ui(
                '这是不可撤销的文件移除，不保证 SSD 物理擦除。加密配对保留在 plaintext-retired/，不属于 ZIP 备份列表，使用本次验证的口令或对应恢复密钥取回。',
                'Removal is irreversible and does not guarantee SSD physical wiping. The verified encrypted pair stays in plaintext-retired/ outside ZIP backup rotation.'
              )}
            </p>
            {residue.blocked ? (
              <p role="alert">
                {ui('存在待处理的迁移、恢复或旧库侧文件，已禁止移除。', 'Pending migration, restore, or sidecar files prevent removal right now.')}
              </p>
            ) : (
              <div className="flex flex-col gap-2">
                {!hasDpapiWrap && (
                  <input
                    aria-label={ui('当前保险库口令', 'Current vault passphrase')}
                    type="password"
                    autoComplete="off"
                    placeholder={ui('当前保险库口令（重新验证）', 'Current vault passphrase (re-verify)')}
                    value={retirementPassword}
                    disabled={isRetiring}
                    onChange={(event) => setRetirementPassword(event.target.value)}
                    className="p-2 rounded border border-slate-300 dark:border-slate-700 bg-white dark:bg-slate-900"
                  />
                )}
                <input
                  aria-label={ui('移除旧副本确认文字', 'Confirmation phrase')}
                  placeholder={ui('输入：移除旧明文副本', 'Type exact phrase: 移除旧明文副本')}
                  value={retirementConfirmation}
                  disabled={isRetiring}
                  onChange={(event) => setRetirementConfirmation(event.target.value)}
                  className="p-2 rounded border border-slate-300 dark:border-slate-700 bg-white dark:bg-slate-900"
                />
                <button
                  type="button"
                  onClick={handleRetirement}
                  disabled={isRetiring || (!hasDpapiWrap && !retirementPassword) || retirementConfirmation !== '移除旧明文副本'}
                  className="self-start px-3 py-2 rounded bg-amber-800 text-white disabled:opacity-50 cursor-pointer"
                >
                  {isRetiring ? ui('正在验证并加密归档…', 'Verifying & archiving...') : ui('加密归档并移除旧明文副本', 'Encrypt & remove plaintext backup')}
                </button>
              </div>
            )}
          </>
        ) : (
          residue && (
            <p>
              {ui(
                '未发现 archive.db.plaintext.bak。此状态不代表没有其他明文导出或历史残留。',
                'archive.db.plaintext.bak was not found. This does not rule out user-created plaintext exports elsewhere.'
              )}
            </p>
          )
        )}
        {residueError && <p role="alert" className="text-rose-700 dark:text-rose-300">{residueError}</p>}
      </div>

      {/* Action Buttons Row */}
      <LegacyArchiveRetirementCard />
      <div className="flex flex-wrap gap-2.5">
        <button
          type="button"
          onClick={() => handleImportBackup().catch(() => showToast(ui('选择备份文件失败', 'Failed to choose backup file'), 'error'))}
          disabled={isRestoring}
          className="px-3 py-2 rounded-lg border border-slate-300 dark:border-slate-700 text-xs cursor-pointer disabled:opacity-50"
        >
          {ui('导入加密备份', 'Import encrypted backup')}
        </button>
        <button
          type="button"
          onClick={handleCreateBackup}
          disabled={isBackingUp}
          className="px-3 py-2 rounded-lg bg-emerald-600 hover:bg-emerald-700 text-white font-medium text-xs flex items-center gap-1.5 shadow-sm transition disabled:opacity-50 cursor-pointer"
        >
          {isBackingUp ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <HardDrive className="w-3.5 h-3.5" />}
          <span>{isBackingUp ? ui('正在备份...', 'Backing up...') : ui('立即备份数据库', 'Create encrypted backup')}</span>
        </button>

        <button
          type="button"
          onClick={handleIntegrityCheck}
          disabled={isChecking}
          className="px-3 py-2 rounded-lg border border-slate-300 dark:border-slate-700 hover:bg-slate-100 dark:hover:bg-slate-700/60 text-slate-700 dark:text-slate-200 font-medium text-xs flex items-center gap-1.5 transition disabled:opacity-50 cursor-pointer"
        >
          {isChecking ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <FileCheck className="w-3.5 h-3.5 text-blue-500" />}
          <span>{isChecking ? ui('正在检查...', 'Checking...') : ui('数据库完整性检查', 'Integrity check')}</span>
        </button>

        <button
          type="button"
          onClick={handleVacuum}
          disabled={isVacuuming}
          className="px-3 py-2 rounded-lg border border-slate-300 dark:border-slate-700 hover:bg-slate-100 dark:hover:bg-slate-700/60 text-slate-700 dark:text-slate-200 font-medium text-xs flex items-center gap-1.5 transition disabled:opacity-50 cursor-pointer"
        >
          {isVacuuming ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Activity className="w-3.5 h-3.5 text-amber-500" />}
          <span>{isVacuuming ? ui('正在整理...', 'Compacting...') : ui('整理与压缩 (VACUUM)', 'Compact (VACUUM)')}</span>
        </button>
      </div>

      {/* Integrity Report Display */}
      {integrityReport && (
        <div className="p-3.5 rounded-lg bg-slate-50 dark:bg-slate-900/60 border border-slate-200 dark:border-slate-800 flex flex-col gap-2 text-xs">
          <div className="flex items-center gap-2 font-semibold">
            {integrityReport.is_healthy ? (
              <>
                <CheckCircle2 className="w-4 h-4 text-emerald-500" />
                <span className="text-emerald-700 dark:text-emerald-400">{ui('数据库结构完好无损', 'Database integrity OK')}</span>
              </>
            ) : (
              <>
                <AlertTriangle className="w-4 h-4 text-amber-500" />
                <span className="text-amber-700 dark:text-amber-400">{ui('检测到异常警告', 'Integrity warning detected')}</span>
              </>
            )}
          </div>
          <div className="grid grid-cols-2 sm:grid-cols-4 gap-2 text-slate-600 dark:text-slate-400 font-mono text-xs pt-1">
            <div className="p-2 bg-white dark:bg-slate-800 rounded border border-slate-200/60 dark:border-slate-700/60">
              <div className="text-slate-400 text-[11px] font-sans">{ui('来源配置数', 'Sources')}</div>
              <div className="text-sm font-bold text-slate-800 dark:text-slate-100">{integrityReport.total_sources}</div>
            </div>
            <div className="p-2 bg-white dark:bg-slate-800 rounded border border-slate-200/60 dark:border-slate-700/60">
              <div className="text-slate-400 text-[11px] font-sans">{ui('独立 URL 数', 'Unique URLs')}</div>
              <div className="text-sm font-bold text-slate-800 dark:text-slate-100">{integrityReport.total_urls}</div>
            </div>
            <div className="p-2 bg-white dark:bg-slate-800 rounded border border-slate-200/60 dark:border-slate-700/60">
              <div className="text-slate-400 text-[11px] font-sans">{ui('总访问记录数', 'Total visits')}</div>
              <div className="text-sm font-bold text-slate-800 dark:text-slate-100">{integrityReport.total_visits}</div>
            </div>
            <div className="p-2 bg-white dark:bg-slate-800 rounded border border-slate-200/60 dark:border-slate-700/60">
              <div className="text-slate-400 text-[11px] font-sans">{ui('FTS 全文索引', 'FTS5 index')}</div>
              <div className="text-sm font-bold text-emerald-600 dark:text-emerald-400 font-sans">
                {integrityReport.fts_synced ? ui('已同步', 'Synced') : ui('需重建', 'Needs rebuild')}
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Backups List */}
      <div className="flex flex-col gap-2 pt-2 border-t border-slate-100 dark:border-slate-800">
        <div className="flex items-center justify-between">
          <span className="font-semibold text-slate-700 dark:text-slate-200">
            {ui('本地备份快照库 (自动保留最新 15 份)', 'Encrypted backup snapshots (retains latest 15)')}
          </span>
          <span className="text-xs text-slate-400">{ui(`共 ${backups.length} 份可用快照`, `${backups.length} available`)}</span>
        </div>

        <div className="border border-slate-200 dark:border-slate-700 rounded-lg overflow-hidden">
          <table className="w-full text-left text-xs">
            <thead className="bg-slate-50 dark:bg-slate-900/60 text-slate-500 border-b border-slate-200 dark:border-slate-700">
              <tr>
                <th className="py-2 px-3">{ui('快照文件名', 'Snapshot file')}</th>
                <th className="py-2 px-3">{ui('体积', 'Size')}</th>
                <th className="py-2 px-3">{ui('备份时间', 'Created at')}</th>
                <th className="py-2 px-3 text-right">{ui('操作', 'Actions')}</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-100 dark:divide-slate-800">
              {backups.length === 0 ? (
                <tr>
                  <td colSpan={4} className="py-4 text-center text-slate-400">
                    {ui('暂无本地备份快照，点击上方按钮即可创建', 'No local backup snapshots yet')}
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
                            <span className="text-[11px] text-slate-400 font-sans">
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
                          <button
                            type="button"
                            onClick={() => {
                              setRestoreTarget(b);
                              setRestoreFile(null);
                              setCredentialMode('current');
                              setCredential('');
                            }}
                            className="px-2 py-1 rounded bg-blue-50 hover:bg-blue-100 dark:bg-blue-950/40 dark:hover:bg-blue-900/60 text-blue-700 dark:text-blue-300 text-xs font-medium flex items-center gap-1 transition cursor-pointer"
                            title={ui('从该备份还原系统数据', 'Restore vault from this backup')}
                          >
                            <RotateCcw className="w-3 h-3" />
                            <span>{ui('恢复', 'Restore')}</span>
                          </button>

                          <button
                            type="button"
                            onClick={() => handleExportBackup(b)}
                            disabled={isExportingId === b.id}
                            className="px-2 py-1 rounded border border-slate-200 dark:border-slate-700 hover:bg-slate-100 dark:hover:bg-slate-700 text-slate-600 dark:text-slate-300 text-xs flex items-center gap-1 transition cursor-pointer disabled:opacity-50"
                            title={ui('导出为独立文件', 'Export backup file')}
                          >
                            {isExportingId === b.id ? (
                              <Loader2 className="w-3 h-3 animate-spin" />
                            ) : (
                              <Download className="w-3 h-3" />
                            )}
                            <span>{ui('导出', 'Export')}</span>
                          </button>

                          {confirmDeleteBackupId === b.id ? (
                            <div className="flex items-center gap-1 ml-1">
                              <button
                                type="button"
                                onClick={() => handleDeleteBackup(b.id)}
                                className="px-2 py-1 rounded bg-rose-600 hover:bg-rose-700 text-white text-xs cursor-pointer"
                              >
                                {ui('确认', 'Delete')}
                              </button>
                              <button
                                type="button"
                                onClick={() => setConfirmDeleteBackupId(null)}
                                className="px-2 py-1 rounded border border-slate-300 dark:border-slate-700 text-slate-600 dark:text-slate-300 text-xs cursor-pointer"
                              >
                                {t('export.cancel')}
                              </button>
                            </div>
                          ) : (
                            <button
                              type="button"
                              onClick={() => setConfirmDeleteBackupId(b.id)}
                              aria-label={ui(`删除备份 ${b.file_name}`, `Delete backup ${b.file_name}`)}
                              className="p-1 text-slate-400 hover:text-rose-500 transition cursor-pointer"
                              title={ui('删除该备份', 'Delete backup')}
                            >
                              <Trash2 className="w-3.5 h-3.5" />
                            </button>
                          )}
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
      {(restoreTarget || restoreFile) && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/60 backdrop-blur-sm p-4 animate-in fade-in duration-150"
          onClick={handleCloseRestoreModal}
          role="presentation"
        >
          <div
            ref={restoreDialogRef}
            role="dialog"
            aria-modal="true"
            aria-labelledby="restore-modal-title"
            tabIndex={-1}
            onClick={(e) => e.stopPropagation()}
            className="w-full max-w-md max-h-[85vh] bg-white dark:bg-slate-800 rounded-2xl border border-slate-200 dark:border-slate-700 shadow-2xl overflow-hidden flex flex-col"
          >
            <div className="p-5 overflow-y-auto modal-scroll flex flex-col gap-3">
              <div className="flex items-start justify-between gap-2">
                <div className="flex items-center gap-3 text-blue-600 dark:text-blue-400">
                  <div className="w-10 h-10 rounded-xl bg-blue-50 dark:bg-blue-950/60 border border-blue-200 dark:border-blue-800/60 flex items-center justify-center shadow-inner">
                    <RotateCcw className="w-5 h-5" />
                  </div>
                  <div>
                    <h3 id="restore-modal-title" className="font-bold text-base text-slate-900 dark:text-slate-100">
                      {ui('确认恢复数据库备份？', 'Restore encrypted vault backup?')}
                    </h3>
                    <p className="text-xs text-slate-500">
                      {ui('安全回滚与状态恢复', 'Verified rollback and credential switch')}
                    </p>
                  </div>
                </div>
                <button
                  type="button"
                  onClick={handleCloseRestoreModal}
                  disabled={isRestoring}
                  aria-label={ui('关闭恢复确认窗口', 'Close restore dialog')}
                  className="p-1 text-slate-400 hover:text-slate-600 dark:hover:text-slate-200 rounded cursor-pointer disabled:opacity-40"
                >
                  <X className="w-4 h-4" />
                </button>
              </div>

              <div className="p-3 bg-slate-50 dark:bg-slate-900/70 rounded-xl border border-slate-200/70 dark:border-slate-800 flex flex-col gap-1 text-xs">
                <div className="text-slate-500">{ui('目标快照:', 'Target snapshot:')}</div>
                <div className="font-mono text-slate-800 dark:text-slate-200 font-semibold break-all">
                  {restoreFile ?? restoreTarget?.file_name}
                </div>
                <div className="text-xs text-slate-400 mt-1">
                  {restoreTarget
                    ? ui(`快照生成时间: ${new Date(restoreTarget.created_at).toLocaleString()}`, `Created: ${new Date(restoreTarget.created_at).toLocaleString()}`)
                    : ui('外部备份：需要该备份的原口令或恢复密钥', 'External backup: requires that backup’s passphrase or recovery key')}
                </div>
              </div>

              <label className="flex flex-col gap-2 text-xs text-slate-700 dark:text-slate-300">
                {ui('恢复凭据', 'Restore credentials')}
                <select
                  value={credentialMode}
                  disabled={isRestoring}
                  onChange={(event) => {
                    setCredentialMode(event.target.value as typeof credentialMode);
                    setCredential('');
                  }}
                  className="p-2 rounded-lg bg-slate-50 dark:bg-slate-900 border border-slate-300 dark:border-slate-700"
                >
                  {(!restoreFile || hasDpapiWrap) && (
                    <option value="current">
                      {hasDpapiWrap
                        ? ui('本机自动密钥 / 当前保险库凭据', 'Local OS key / current vault credentials')
                        : ui('当前保险库凭据（须与备份信封完全匹配）', 'Current vault credentials (must match backup envelope)')}
                    </option>
                  )}
                  <option value="password">{ui('备份时的原口令', 'Original backup passphrase')}</option>
                  <option value="recovery">{ui('备份的恢复密钥', 'Backup recovery key')}</option>
                </select>
                {credentialMode !== 'current' && (
                  <input
                    type="password"
                    autoComplete="off"
                    value={credential}
                    disabled={isRestoring}
                    onChange={(event) => setCredential(event.target.value)}
                    placeholder={
                      credentialMode === 'password'
                        ? ui('输入备份时使用的口令', 'Enter backup passphrase')
                        : ui('输入 BHA-... 恢复密钥', 'Enter BHA-... recovery key')
                    }
                    className="p-2 rounded-lg bg-slate-50 dark:bg-slate-900 border border-slate-300 dark:border-slate-700"
                  />
                )}
                <span className="text-slate-500">
                  {ui(
                    '校验通过后保留旧数据的加密副本；恢复完成后保险库将锁定，请用备份凭据重新解锁。',
                    'An encrypted pre-restore copy is retained. After restore finishes, the vault locks so you can unlock with the restored credentials.'
                  )}
                </span>
              </label>

              {/* Safety Guarantee Alert */}
              <div className="p-3 bg-emerald-50/70 dark:bg-emerald-950/30 rounded-xl border border-emerald-200/80 dark:border-emerald-900/60 text-emerald-800 dark:text-emerald-300 text-xs flex items-start gap-2.5">
                <ShieldCheck className="w-4 h-4 text-emerald-600 dark:text-emerald-400 shrink-0 mt-0.5" />
                <div className="flex flex-col gap-0.5 leading-relaxed text-xs">
                  <span className="font-semibold">{ui('前置快照保护机制：', 'Pre-restore safeguard:')}</span>
                  <span>
                    {ui(
                      '切换前会校验并保留 backups/pre_restore_*/ 中的加密数据库与信封配对副本。恢复中断时按状态记录回滚；这些副本不属于列表中的 ZIP 备份。',
                      'Saves a verified encrypted archive and envelope pair in backups/pre_restore_*/ before switching. Interrupted restores roll back cleanly.'
                    )}
                  </span>
                </div>
              </div>
            </div>

            <div className="p-4 bg-slate-50 dark:bg-slate-800 border-t border-slate-200 dark:border-slate-700/80 flex items-center justify-end gap-2.5">
              <button
                type="button"
                onClick={handleCloseRestoreModal}
                disabled={isRestoring}
                className="px-3.5 py-1.5 rounded-lg border border-slate-300 dark:border-slate-700 hover:bg-slate-100 dark:hover:bg-slate-800 text-slate-700 dark:text-slate-300 text-xs font-medium transition cursor-pointer disabled:opacity-40"
              >
                {t('export.cancel')}
              </button>
              <button
                type="button"
                onClick={handleRestoreConfirm}
                disabled={isRestoring || (credentialMode !== 'current' && !credential)}
                className="px-4 py-1.5 rounded-lg bg-blue-600 hover:bg-blue-700 text-white text-xs font-semibold shadow-md flex items-center gap-1.5 transition cursor-pointer disabled:opacity-50"
              >
                {isRestoring ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <RotateCcw className="w-3.5 h-3.5" />}
                <span>{isRestoring ? ui('正在还原数据库...', 'Restoring...') : ui('确认恢复', 'Confirm restore')}</span>
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};
