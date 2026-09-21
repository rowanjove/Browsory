import React, { useState } from 'react';
import { Minus, Power } from 'lucide-react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { tauriApi } from '../../services/tauri';
import { useAppStore } from '../../stores/useAppStore';

interface CloseConfirmModalProps {
  isOpen: boolean;
  onClose: () => void;
}

export const CloseConfirmModal: React.FC<CloseConfirmModalProps> = ({ isOpen, onClose }) => {
  const { showToast } = useAppStore();
  const [remember, setRemember] = useState(false);
  const [busy, setBusy] = useState(false);

  if (!isOpen) return null;

  const persist = async (value: 'tray' | 'quit') => {
    if (!remember) return;
    try {
      await tauriApi.setSetting('close_behavior', value);
    } catch (err) {
      showToast(typeof err === 'string' ? err : '无法保存关闭行为', 'error');
    }
  };

  const handleTray = async () => {
    setBusy(true);
    await persist('tray');
    try {
      await getCurrentWindow().hide();
      onClose();
    } catch (err) {
      showToast(typeof err === 'string' ? err : '无法最小化到托盘', 'error');
    } finally {
      setBusy(false);
    }
  };

  const handleQuit = async () => {
    setBusy(true);
    await persist('quit');
    try {
      await tauriApi.quitApplication();
    } catch (err) {
      showToast(typeof err === 'string' ? err : '无法退出应用', 'error');
      setBusy(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/50 backdrop-blur-sm p-4">
      <div className="w-full max-w-sm bg-white dark:bg-slate-900 border border-slate-200 dark:border-slate-800 rounded-2xl shadow-2xl p-5 space-y-4">
        <div>
          <h3 className="text-sm font-bold text-slate-900 dark:text-slate-100">关闭 Browsory</h3>
          <p className="text-xs text-slate-500 dark:text-slate-400 mt-1 leading-relaxed">
            点窗口关闭默认不会退出。后台仍会监视浏览器历史。请选择本次操作：
          </p>
        </div>

        <label className="flex items-center gap-2 text-xs text-slate-600 dark:text-slate-300">
          <input
            type="checkbox"
            checked={remember}
            onChange={(e) => setRemember(e.target.checked)}
            className="rounded border-slate-300"
          />
          记住我的选择
        </label>

        <div className="flex justify-end gap-2">
          <button
            type="button"
            disabled={busy}
            onClick={onClose}
            className="px-3 py-1.5 rounded-lg border border-slate-300 dark:border-slate-700 text-xs text-slate-600 dark:text-slate-300"
          >
            取消
          </button>
          <button
            type="button"
            disabled={busy}
            onClick={handleTray}
            className="px-3 py-1.5 rounded-lg border border-slate-300 dark:border-slate-700 text-xs text-slate-700 dark:text-slate-200 flex items-center gap-1.5"
          >
            <Minus className="w-3.5 h-3.5" />
            最小化到托盘
          </button>
          <button
            type="button"
            disabled={busy}
            onClick={handleQuit}
            className="px-3 py-1.5 rounded-lg bg-slate-800 hover:bg-slate-900 text-white text-xs flex items-center gap-1.5"
          >
            <Power className="w-3.5 h-3.5" />
            退出
          </button>
        </div>
      </div>
    </div>
  );
};
