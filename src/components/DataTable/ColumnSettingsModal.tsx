import React from 'react';
import { X } from 'lucide-react';
import { useHistoryStore, ColumnVisibility } from '../../stores/useHistoryStore';
import { useAppStore } from '../../stores/useAppStore';

interface Props {
  isOpen: boolean;
  onClose: () => void;
}

export const ColumnSettingsModal: React.FC<Props> = ({ isOpen, onClose }) => {
  const { columns, toggleColumn } = useHistoryStore();
  const { t } = useAppStore();

  if (!isOpen) return null;

  const columnOptions: { key: keyof ColumnVisibility; label: string; required?: boolean }[] = [
    { key: 'time', label: t('history.columns.time'), required: true },
    { key: 'title', label: t('history.columns.title'), required: true },
    { key: 'url', label: t('history.columns.url'), required: true },
    { key: 'domain', label: t('history.columns.domain') },
    { key: 'browser', label: t('history.columns.browser') },
    { key: 'profile', label: t('history.columns.profile') },
    { key: 'duration', label: t('history.columns.duration') },
  ];

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-[1px] animate-in fade-in duration-100">
      <div className="w-80 bg-white dark:bg-slate-800 rounded border border-slate-300 dark:border-slate-700 shadow-xl p-4 flex flex-col gap-3 text-xs">
        <div className="flex items-center justify-between border-b border-slate-200 dark:border-slate-700 pb-2">
          <span className="font-semibold text-slate-800 dark:text-slate-100">{t('history.columnSettings')}</span>
          <button onClick={onClose} className="p-1 hover:bg-slate-100 dark:hover:bg-slate-700 rounded">
            <X className="w-4 h-4 text-slate-500" />
          </button>
        </div>

        <div className="flex flex-col gap-1.5 py-1">
          {columnOptions.map((opt) => {
            const isChecked = columns[opt.key];
            return (
              <label
                key={opt.key}
                className={`flex items-center justify-between px-2.5 py-1.5 rounded cursor-pointer transition ${
                  opt.required ? 'opacity-60 cursor-not-allowed bg-slate-50 dark:bg-slate-800/50' : 'hover:bg-slate-100 dark:hover:bg-slate-700'
                }`}
              >
                <span className="text-slate-700 dark:text-slate-200">
                  {opt.label}
                  {opt.required && <span className="ml-1 text-[10px] text-slate-400">(默认必选)</span>}
                </span>
                <input
                  type="checkbox"
                  checked={isChecked}
                  disabled={opt.required}
                  onChange={() => !opt.required && toggleColumn(opt.key)}
                  className="rounded border-slate-300 text-blue-600 focus:ring-0 focus:ring-offset-0"
                />
              </label>
            );
          })}
        </div>

        <div className="flex justify-end pt-2 border-t border-slate-200 dark:border-slate-700">
          <button
            onClick={onClose}
            className="px-3 py-1 rounded bg-blue-600 text-white hover:bg-blue-700 transition"
          >
            完成
          </button>
        </div>
      </div>
    </div>
  );
};
