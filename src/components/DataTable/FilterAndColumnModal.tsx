import React from 'react';
import { X, Check, Filter, Columns3, RotateCcw, Compass } from 'lucide-react';
import { useHistoryStore, ColumnVisibility } from '../../stores/useHistoryStore';
import { useAppStore } from '../../stores/useAppStore';
import { browserFilterOptions } from '../../services/browserMetadata';
import { useDialogFocus } from '../../hooks/useDialogFocus';

interface Props {
  isOpen: boolean;
  onClose: () => void;
  availableProfiles: string[];
  availableBrowsers: string[];
}

export const FilterAndColumnModal: React.FC<Props> = ({
  isOpen,
  onClose,
  availableProfiles,
  availableBrowsers,
}) => {
  const {
    selectedBrowsers,
    setSelectedBrowsers,
    selectedProfiles,
    setSelectedProfiles,
    columns,
    toggleColumn,
  } = useHistoryStore();
  const { t, language } = useAppStore();
  const ui = (zh: string, en: string) => language === 'en-US' ? en : zh;
  const dialogRef = useDialogFocus<HTMLDivElement>({ isOpen, onClose });
  if (!isOpen) return null;
  const browserOptions = browserFilterOptions([...availableBrowsers, ...selectedBrowsers]);

  const columnOptions: { key: keyof ColumnVisibility; label: string; required?: boolean }[] = [
    { key: 'time', label: t('history.columns.time'), required: true },
    { key: 'title', label: t('history.columns.title'), required: true },
    { key: 'url', label: t('history.columns.url'), required: true },
    { key: 'domain', label: t('history.columns.domain') },
    { key: 'browser', label: t('history.columns.browser') },
    { key: 'profile', label: t('history.columns.profile') },
    { key: 'duration', label: t('history.columns.duration') },
  ];

  const handleResetFilters = () => {
    setSelectedBrowsers([]);
    setSelectedProfiles([]);
  };

  const isAllBrowsers = selectedBrowsers.length === 0;
  const isAllProfiles = selectedProfiles.length === 0;

  const toggleBrowser = (browserId: string) => {
    if (selectedBrowsers.includes(browserId)) {
      const next = selectedBrowsers.filter((b) => b !== browserId);
      setSelectedBrowsers(next);
    } else {
      setSelectedBrowsers([...selectedBrowsers, browserId]);
    }
  };

  const toggleProfile = (profileName: string) => {
    if (selectedProfiles.includes(profileName)) {
      const next = selectedProfiles.filter((p) => p !== profileName);
      setSelectedProfiles(next);
    } else {
      setSelectedProfiles([...selectedProfiles, profileName]);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-xs animate-in fade-in duration-100 p-4">
      <div ref={dialogRef} role="dialog" aria-modal="true" aria-labelledby="history-filter-title" tabIndex={-1} className="w-full max-w-lg max-h-[calc(100vh-2rem)] bg-white dark:bg-slate-800 rounded-xl border border-slate-200 dark:border-slate-700 shadow-2xl overflow-hidden flex flex-col text-xs">
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-slate-200 dark:border-slate-700 bg-slate-50/50 dark:bg-slate-800/50">
          <div className="flex items-center gap-2">
            <Filter className="w-4 h-4 text-blue-500" />
            <span id="history-filter-title" className="font-semibold text-slate-800 dark:text-slate-100 text-sm">
              {ui('筛选与表格视图设置', 'Filters and table view')}
            </span>
          </div>
          <button
            onClick={onClose}
            aria-label={ui('关闭筛选窗口', 'Close filters')}
            className="p-1 hover:bg-slate-100 dark:hover:bg-slate-700 rounded text-slate-400 hover:text-slate-600 transition"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Body */}
        <div className="p-4 flex-1 min-h-0 flex flex-col gap-4 overflow-y-auto">
          {/* 1. Browser filter */}
          <div className="flex flex-col gap-2">
            <div className="flex items-center justify-between">
              <label className="font-medium text-slate-700 dark:text-slate-200 flex items-center gap-1.5">
                <Compass className="w-3.5 h-3.5 text-slate-400" />
                <span>{ui('浏览器来源', 'Browser sources')}</span>
              </label>
              {selectedBrowsers.length > 0 && (
                <button
                  type="button"
                  onClick={() => setSelectedBrowsers([])}
                  className="text-[11px] text-blue-600 dark:text-blue-400 hover:underline"
                >
                  {ui('重置为全部', 'Show all')}
                </button>
              )}
            </div>

            <div className="grid grid-cols-2 gap-1.5">
              <button
                type="button"
                onClick={() => setSelectedBrowsers([])}
                className={`flex items-center justify-between px-3 py-2 rounded-lg border text-left transition ${
                  isAllBrowsers
                    ? 'border-blue-500 bg-blue-50/60 dark:bg-blue-950/40 text-blue-700 dark:text-blue-300 font-medium ring-1 ring-blue-500/20'
                    : 'border-slate-200 dark:border-slate-700 bg-slate-50/50 dark:bg-slate-900/40 text-slate-700 dark:text-slate-300 hover:bg-slate-100 dark:hover:bg-slate-800'
                }`}
              >
                <span>{ui('全部浏览器', 'All browsers')}</span>
                {isAllBrowsers && <Check className="w-3.5 h-3.5 text-blue-600" />}
              </button>

              {browserOptions.map((b) => {
                const isSelected = selectedBrowsers.includes(b.id);
                return (
                  <button
                    key={b.id}
                    type="button"
                    onClick={() => toggleBrowser(b.id)}
                    className={`flex items-center justify-between px-3 py-2 rounded-lg border text-left transition ${
                      isSelected
                        ? 'border-blue-500 bg-blue-50/60 dark:bg-blue-950/40 text-blue-700 dark:text-blue-300 font-medium ring-1 ring-blue-500/20'
                        : 'border-slate-200 dark:border-slate-700 bg-slate-50/50 dark:bg-slate-900/40 text-slate-700 dark:text-slate-300 hover:bg-slate-100 dark:hover:bg-slate-800'
                    }`}
                  >
                    <span>{b.label}</span>
                    {isSelected && <Check className="w-3.5 h-3.5 text-blue-600" />}
                  </button>
                );
              })}
            </div>
          </div>

          {/* 2. Profiles filter */}
          {availableProfiles.length > 0 && (
            <div className="flex flex-col gap-2 pt-2 border-t border-slate-100 dark:border-slate-700/60">
              <div className="flex items-center justify-between">
                <label className="font-medium text-slate-700 dark:text-slate-200 flex items-center gap-1.5">
                  <span>{ui('用户配置文件 (Profile)', 'Browser profiles')}</span>
                </label>
                {selectedProfiles.length > 0 && (
                  <button
                    type="button"
                    onClick={() => setSelectedProfiles([])}
                    className="text-[11px] text-blue-600 dark:text-blue-400 hover:underline"
                  >
                    {ui('重置为全部', 'Show all')}
                  </button>
                )}
              </div>

              <div className="flex flex-wrap gap-1.5">
                <button
                  type="button"
                  onClick={() => setSelectedProfiles([])}
                  className={`px-3 py-1.5 rounded-lg border text-xs transition ${
                    isAllProfiles
                      ? 'border-blue-500 bg-blue-50/60 dark:bg-blue-950/40 text-blue-700 dark:text-blue-300 font-medium'
                      : 'border-slate-200 dark:border-slate-700 bg-slate-50/50 dark:bg-slate-900/40 text-slate-700 dark:text-slate-300 hover:bg-slate-100 dark:hover:bg-slate-800'
                  }`}
                >
                  {ui('全部配置', 'All profiles')}
                </button>
                {availableProfiles.map((p) => {
                  const isSelected = selectedProfiles.includes(p);
                  return (
                    <button
                      key={p}
                      type="button"
                      onClick={() => toggleProfile(p)}
                      className={`px-3 py-1.5 rounded-lg border text-xs transition flex items-center gap-1.5 ${
                        isSelected
                          ? 'border-blue-500 bg-blue-50/60 dark:bg-blue-950/40 text-blue-700 dark:text-blue-300 font-medium'
                          : 'border-slate-200 dark:border-slate-700 bg-slate-50/50 dark:bg-slate-900/40 text-slate-700 dark:text-slate-300 hover:bg-slate-100 dark:hover:bg-slate-800'
                      }`}
                    >
                      <span>{p}</span>
                      {isSelected && <Check className="w-3 h-3 text-blue-600" />}
                    </button>
                  );
                })}
              </div>
            </div>
          )}

          {/* 3. Column visibility */}
          <div className="flex flex-col gap-2 pt-2 border-t border-slate-100 dark:border-slate-700/60">
            <label className="font-medium text-slate-700 dark:text-slate-200 flex items-center gap-1.5">
              <Columns3 className="w-3.5 h-3.5 text-slate-400" />
              <span>{ui('表格列显示', 'Visible columns')}</span>
            </label>

            <div className="grid grid-cols-2 gap-1.5">
              {columnOptions.map((opt) => {
                const isChecked = columns[opt.key];
                return (
                  <label
                    key={opt.key}
                    className={`flex items-center justify-between px-3 py-2 rounded-lg border transition cursor-pointer ${
                      opt.required
                        ? 'opacity-60 cursor-not-allowed bg-slate-50 dark:bg-slate-800/50 border-slate-200 dark:border-slate-700'
                        : isChecked
                        ? 'border-blue-500/50 bg-blue-50/30 dark:bg-blue-950/20 border-slate-200 dark:border-slate-700'
                        : 'border-slate-200 dark:border-slate-700 hover:bg-slate-100 dark:hover:bg-slate-700'
                    }`}
                  >
                    <span className="text-slate-700 dark:text-slate-200">
                      {opt.label}
                      {opt.required && (
                        <span className="ml-1 text-[10px] text-slate-400">{ui('(必显)', '(Required)')}</span>
                      )}
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
          </div>
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between px-4 py-3 border-t border-slate-200 dark:border-slate-700 bg-slate-50/50 dark:bg-slate-800/50">
          <button
            type="button"
            onClick={handleResetFilters}
            className="flex items-center gap-1 px-2.5 py-1.5 rounded-lg border border-slate-200 dark:border-slate-700 hover:bg-slate-100 dark:hover:bg-slate-700 text-slate-600 dark:text-slate-300 transition"
          >
            <RotateCcw className="w-3.5 h-3.5" />
            <span>{ui('重置筛选', 'Reset filters')}</span>
          </button>

          <button
            type="button"
            onClick={onClose}
            className="px-4 py-1.5 rounded-lg bg-blue-600 text-white hover:bg-blue-700 transition font-medium cursor-pointer"
          >
            {ui('完成', 'Done')}
          </button>
        </div>
      </div>
    </div>
  );
};
