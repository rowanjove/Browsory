import React, { useState } from 'react';
import { AlertCircle, Loader2 } from 'lucide-react';
import { useAppStore } from '../../stores/useAppStore';
import { tauriApi } from '../../services/tauri';
import { useHistoryStore } from '../../stores/useHistoryStore';

export const RunningModal: React.FC = () => {
  const { runningModal, closeRunningModal, showToast, t } = useAppStore();
  const { fetchHistory } = useHistoryStore();
  const [loadingAction, setLoadingAction] = useState<'try' | 'close' | null>(null);

  if (!runningModal.isOpen) return null;

  const handleTrySync = async () => {
    setLoadingAction('try');
    try {
      if (runningModal.sourceId) {
        const res = await tauriApi.syncSource(runningModal.sourceId);
        showToast(t('sources.syncSuccess', { inserted: res.inserted_count, duplicate: res.duplicate_count }), 'success');
      } else {
        const results = await tauriApi.syncAll();
        const inserted = results.reduce((acc, r) => acc + r.inserted_count, 0);
        const duplicate = results.reduce((acc, r) => acc + r.duplicate_count, 0);
        showToast(t('sources.syncSuccess', { inserted, duplicate }), 'success');
      }
      closeRunningModal();
      fetchHistory(true);
    } catch (err: any) {
      showToast(err?.message || '无法稳定读取被占用的数据库，请关闭浏览器后重试', 'error');
    } finally {
      setLoadingAction(null);
    }
  };

  const handleCloseAndSync = async () => {
    setLoadingAction('close');
    try {
      // 1. Terminate browser gracefully
      await tauriApi.killBrowser(runningModal.browser, false);
      // Wait 1.5s for process cleanup
      await new Promise((r) => setTimeout(r, 1500));

      // 2. Perform sync
      if (runningModal.sourceId) {
        const res = await tauriApi.syncSource(runningModal.sourceId);
        showToast(t('sources.syncSuccess', { inserted: res.inserted_count, duplicate: res.duplicate_count }), 'success');
      } else {
        const results = await tauriApi.syncAll();
        const inserted = results.reduce((acc, r) => acc + r.inserted_count, 0);
        const duplicate = results.reduce((acc, r) => acc + r.duplicate_count, 0);
        showToast(t('sources.syncSuccess', { inserted, duplicate }), 'success');
      }
      closeRunningModal();
      fetchHistory(true);
    } catch (err: any) {
      // If graceful failed, prompt force kill
      showToast('关闭浏览器或同步失败，可能需要再次尝试或手动关闭', 'error');
    } finally {
      setLoadingAction(null);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-[1px] animate-in fade-in duration-150">
      <div className="w-[420px] bg-white dark:bg-slate-800 rounded border border-slate-300 dark:border-slate-700 shadow-xl p-5 flex flex-col gap-4 text-xs">
        <div className="flex items-start gap-3">
          <div className="p-2 rounded bg-amber-100 text-amber-600 dark:bg-amber-950 dark:text-amber-400 shrink-0">
            <AlertCircle className="w-5 h-5" />
          </div>
          <div className="flex flex-col gap-1">
            <h3 className="text-sm font-semibold text-slate-800 dark:text-slate-100">
              {t('processModal.title')}
            </h3>
            <p className="text-slate-600 dark:text-slate-300 leading-relaxed">
              {t('processModal.desc', { browser: runningModal.browser })}
            </p>
            <p className="text-[11px] text-amber-700 dark:text-amber-400 leading-relaxed">
              「关闭浏览器后再同步」会结束该浏览器的所有窗口，未保存的网页可能丢失。
            </p>
          </div>
        </div>

        <div className="flex items-center justify-end gap-2 pt-2 border-t border-slate-200 dark:border-slate-700">
          <button
            onClick={closeRunningModal}
            disabled={loadingAction !== null}
            className="px-3 py-1.5 rounded border border-slate-300 dark:border-slate-600 text-slate-700 dark:text-slate-200 hover:bg-slate-100 dark:hover:bg-slate-700 transition"
          >
            {t('processModal.cancel')}
          </button>
          <button
            onClick={handleTrySync}
            disabled={loadingAction !== null}
            className="px-3 py-1.5 rounded bg-slate-200 dark:bg-slate-700 text-slate-800 dark:text-slate-100 hover:bg-slate-300 dark:hover:bg-slate-600 transition flex items-center gap-1.5"
          >
            {loadingAction === 'try' && <Loader2 className="w-3.5 h-3.5 animate-spin" />}
            {t('processModal.trySync')}
          </button>
          <button
            onClick={handleCloseAndSync}
            disabled={loadingAction !== null}
            className="px-3 py-1.5 rounded bg-blue-600 text-white hover:bg-blue-700 transition flex items-center gap-1.5"
          >
            {loadingAction === 'close' && <Loader2 className="w-3.5 h-3.5 animate-spin" />}
            {t('processModal.closeAndSync')}
          </button>
        </div>
      </div>
    </div>
  );
};
