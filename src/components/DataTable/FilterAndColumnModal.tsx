import React from 'react';
import { X, Check, Filter, Columns3, RotateCcw, Compass } from 'lucide-react';
import { useHistoryStore, ColumnVisibility } from '../../stores/useHistoryStore';
import { useAppStore } from '../../stores/useAppStore';

interface Props {
  isOpen: boolean;
  onClose: () => void;
  availableProfiles: string[];
}

export const FilterAndColumnModal: React.FC<Props> = ({
  isOpen,
  onClose,
  availableProfiles,
}) => {
  const {
    selectedBrowsers,
    setSelectedBrowsers,
    selectedProfiles,
    setSelectedProfiles,
    columns,
    toggleColumn,
  } = useHistoryStore();
  const { t } = useAppStore();

  if (!isOpen) return null;

  const browserOptions = [
    { id: 'chrome', label: 'Google Chrome' },
    { id: 'edge', label: 'Microsoft Edge' },
    { id: 'brave', label: 'Brave Browser' },
    { id: 'vivaldi', label: 'Vivaldi' },
  ];

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
      <div className="w-full max-w-lg bg-white dark:bg-slate-800 rounded-xl border border-slate-200 dark:border-slate-700 shadow-2xl overflow-hidden flex flex-col text-xs">
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-slate-200 dark:border-slate-700 bg-slate-50/50 dark:bg-slate-800/50">
          <div className="flex items-center gap-2">
            <Filter className="w-4 h-4 text-blue-500" />
            <span className="font-semibold text-slate-800 dark:text-slate-100 text-sm">
              筛选与表格视图设置
            </span>
          </div>
          <button
            onClick={onClose}
            className="p-1 hover:bg-slate-100 dark:hover:bg-slate-700 rounded text-slate-400 hover:text-slate-600 transition"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Body */}
        <div className="p-4 flex flex-col gap-4 max-h-[75vh] overflow-y-auto">
          {/* 1. Browser filter */}
          <div className="flex flex-col gap-2">
            <div className="flex items-center justify-between">
              <label className="font-medium text-slate-700 dark:text-slate-200 flex items-center gap-1.5">
                <Compass className="w-3.5 h-3.5 text-slate-400" />
                <span>浏览器来源</span>
              </label>
              {selectedBrowsers.length > 0 && (
                <button
                  type="button"
                  onClick={() => setSelectedBrowsers([])}
                  className="text-[11px] text-blue-600 dark:text-blue-400 hover:underline"
                >
                  重置为全部
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
                <span>全部浏览器</span>
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
                  <span>用户配置文件 (Profile)</span>
                </label>
                {selectedProfiles.length > 0 && (
                  <button
                    type="button"
                    onClick={() => setSelectedProfiles([])}
                    className="text-[11px] text-blue-600 dark:text-blue-400 hover:underline"
                  >
                    重置为全部
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
                  全部配置
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
              <span>表格列显示</span>
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
                        <span className="ml-1 text-[10px] text-slate-400">(必显)</span>
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
            <span>重置筛选</span>
          </button>

          <button
            type="button"
            onClick={onClose}
            className="px-4 py-1.5 rounded-lg bg-blue-600 text-white hover:bg-blue-700 transition font-medium cursor-pointer"
          >
            完成
          </button>
        </div>
      </div>
    </div>
  );
};
