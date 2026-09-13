import React, { useState } from 'react';
import { X, Download, FileSpreadsheet, FileText, Code2, Loader2 } from 'lucide-react';
import { save } from '@tauri-apps/plugin-dialog';
import { useHistoryStore, calculateTimeRange } from '../../stores/useHistoryStore';
import { useAppStore } from '../../stores/useAppStore';
import { tauriApi } from '../../services/tauri';
import { ExportOptions, HistoryFilter } from '../../types';

interface ExportModalProps {
  isOpen: boolean;
  onClose: () => void;
}

export const ExportModal: React.FC<ExportModalProps> = ({ isOpen, onClose }) => {
  const {
    search,
    selectedBrowsers,
    selectedProfiles,
    selectedIds,
    timeRange,
    customStartTime,
    customEndTime,
    selectedDate,
    onlyFavorites,
    selectedTag,
    sortBy,
  } = useHistoryStore();
  const { showToast } = useAppStore();

  const [format, setFormat] = useState<'csv' | 'xlsx' | 'json'>('csv');
  const [scope, setScope] = useState<'all' | 'filtered' | 'selected'>(
    selectedIds.size > 0 ? 'selected' : 'filtered'
  );
  const [isExporting, setIsExporting] = useState(false);

  const [columns, setColumns] = useState<Record<string, boolean>>({
    time: true,
    title: true,
    url: true,
    domain: true,
    browser: true,
    profile: true,
    duration: true,
  });

  if (!isOpen) return null;

  const toggleColumn = (key: string) => {
    setColumns((prev) => ({ ...prev, [key]: !prev[key] }));
  };

  const handleExport = async () => {
    setIsExporting(true);
    try {
      // 1. Choose default file name
      const nowStr = new Date().toISOString().slice(0, 10);
      const defaultPath = `browser-history-${nowStr}.${format}`;

      const extMap = {
        csv: [{ name: 'CSV 文件', extensions: ['csv'] }],
        xlsx: [{ name: 'Excel 电子表格', extensions: ['xlsx'] }],
        json: [{ name: 'JSON 数据文件', extensions: ['json'] }],
      };

      const selectedPath = await save({
        defaultPath,
        filters: extMap[format],
      });

      if (!selectedPath) {
        setIsExporting(false);
        return;
      }

      // 2. Prepare filter and options
      const activeColumns = Object.keys(columns).filter((k) => columns[k]);

      let visitIds: number[] | undefined;
      let filter: HistoryFilter | undefined;

      if (scope === 'selected') {
        visitIds = Array.from(selectedIds);
      } else if (scope === 'filtered') {
        let startTime: number | undefined;
        let endTime: number | undefined;

        if (selectedDate) {
          const [y, m, d] = selectedDate.split('-').map(Number);
          const dayStart = new Date(y, m - 1, d, 0, 0, 0, 0);
          const dayEnd = new Date(y, m - 1, d, 23, 59, 59, 999);
          startTime = dayStart.getTime();
          endTime = dayEnd.getTime();
        } else if (timeRange === 'custom') {
          startTime = customStartTime || undefined;
          endTime = customEndTime || undefined;
        } else {
          [startTime, endTime] = calculateTimeRange(timeRange);
        }

        filter = {
          search: search.trim() || undefined,
          browsers: selectedBrowsers.length > 0 ? selectedBrowsers : undefined,
          profiles: selectedProfiles.length > 0 ? selectedProfiles : undefined,
          start_time: startTime,
          end_time: endTime,
          only_favorites: onlyFavorites ? true : undefined,
          tag: selectedTag || undefined,
          sort_by: sortBy,
        };
      }

      const options: ExportOptions = {
        format,
        file_path: selectedPath,
        columns: activeColumns,
        visit_ids: visitIds,
        filter,
      };

      const count = await tauriApi.exportHistory(options);
      showToast(`导出成功！已将 ${count} 条记录保存至本地`, 'success');
      onClose();
    } catch (err: any) {
      showToast(err?.message || '导出失败', 'error');
    } finally {
      setIsExporting(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-[1px] animate-in fade-in duration-150">
      <div className="w-[460px] bg-white dark:bg-slate-800 rounded border border-slate-300 dark:border-slate-700 shadow-xl p-5 flex flex-col gap-4 text-xs select-none">
        {/* Modal Header */}
        <div className="flex items-center justify-between pb-3 border-b border-slate-200 dark:border-slate-700">
          <div className="flex items-center gap-2 font-semibold text-slate-800 dark:text-slate-100 text-sm">
            <Download className="w-4 h-4 text-blue-500" />
            <span>导出历史数据</span>
          </div>
          <button onClick={onClose} className="p-1 hover:bg-slate-100 dark:hover:bg-slate-700 rounded text-slate-500">
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Format Selection */}
        <div className="flex flex-col gap-1.5">
          <span className="text-slate-600 dark:text-slate-400 font-medium">导出格式</span>
          <div className="grid grid-cols-3 gap-2">
            <button
              onClick={() => setFormat('csv')}
              className={`flex items-center justify-center gap-1.5 py-2 px-3 rounded border transition ${
                format === 'csv'
                  ? 'border-blue-500 bg-blue-50/50 dark:bg-blue-950/40 text-blue-600 dark:text-blue-400 font-medium'
                  : 'border-slate-200 dark:border-slate-700 bg-slate-50 dark:bg-slate-800 text-slate-700 dark:text-slate-300'
              }`}
            >
              <FileText className="w-3.5 h-3.5" />
              <span>CSV (.csv)</span>
            </button>
            <button
              onClick={() => setFormat('xlsx')}
              className={`flex items-center justify-center gap-1.5 py-2 px-3 rounded border transition ${
                format === 'xlsx'
                  ? 'border-blue-500 bg-blue-50/50 dark:bg-blue-950/40 text-blue-600 dark:text-blue-400 font-medium'
                  : 'border-slate-200 dark:border-slate-700 bg-slate-50 dark:bg-slate-800 text-slate-700 dark:text-slate-300'
              }`}
            >
              <FileSpreadsheet className="w-3.5 h-3.5" />
              <span>Excel (.xlsx)</span>
            </button>
            <button
              onClick={() => setFormat('json')}
              className={`flex items-center justify-center gap-1.5 py-2 px-3 rounded border transition ${
                format === 'json'
                  ? 'border-blue-500 bg-blue-50/50 dark:bg-blue-950/40 text-blue-600 dark:text-blue-400 font-medium'
                  : 'border-slate-200 dark:border-slate-700 bg-slate-50 dark:bg-slate-800 text-slate-700 dark:text-slate-300'
              }`}
            >
              <Code2 className="w-3.5 h-3.5" />
              <span>JSON (.json)</span>
            </button>
          </div>
        </div>

        {/* Scope Selection */}
        <div className="flex flex-col gap-1.5">
          <span className="text-slate-600 dark:text-slate-400 font-medium">导出范围</span>
          <div className="flex flex-col gap-1.5 bg-slate-50 dark:bg-slate-800/60 p-2.5 rounded border border-slate-200 dark:border-slate-700">
            <label className="flex items-center gap-2 cursor-pointer">
              <input
                type="radio"
                name="scope"
                checked={scope === 'filtered'}
                onChange={() => setScope('filtered')}
                className="text-blue-600"
              />
              <span className="text-slate-700 dark:text-slate-200">当前筛选与搜索结果</span>
            </label>
            <label className="flex items-center gap-2 cursor-pointer">
              <input
                type="radio"
                name="scope"
                checked={scope === 'selected'}
                disabled={selectedIds.size === 0}
                onChange={() => setScope('selected')}
                className="text-blue-600 disabled:opacity-40"
              />
              <span className={`text-slate-700 dark:text-slate-200 ${selectedIds.size === 0 ? 'opacity-40' : ''}`}>
                当前勾选的项目 ({selectedIds.size} 项)
              </span>
            </label>
            <label className="flex items-center gap-2 cursor-pointer">
              <input
                type="radio"
                name="scope"
                checked={scope === 'all'}
                onChange={() => setScope('all')}
                className="text-blue-600"
              />
              <span className="text-slate-700 dark:text-slate-200">全部本地归档历史</span>
            </label>
          </div>
        </div>

        {/* Column Selection (Only for CSV & XLSX) */}
        {format !== 'json' && (
          <div className="flex flex-col gap-1.5">
            <span className="text-slate-600 dark:text-slate-400 font-medium">导出字段</span>
            <div className="grid grid-cols-3 gap-2 bg-slate-50 dark:bg-slate-800/60 p-2.5 rounded border border-slate-200 dark:border-slate-700">
              {[
                { key: 'time', label: '访问时间' },
                { key: 'title', label: '网页标题' },
                { key: 'url', label: 'URL' },
                { key: 'domain', label: '域名' },
                { key: 'browser', label: '浏览器' },
                { key: 'profile', label: '配置文件' },
                { key: 'duration', label: '停留时长' },
              ].map((col) => (
                <label key={col.key} className="flex items-center gap-1.5 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={columns[col.key]}
                    onChange={() => toggleColumn(col.key)}
                    className="rounded border-slate-300 text-blue-600 focus:ring-0"
                  />
                  <span className="text-slate-700 dark:text-slate-300">{col.label}</span>
                </label>
              ))}
            </div>
          </div>
        )}

        {/* Actions */}
        <div className="flex items-center justify-end gap-2 pt-2 border-t border-slate-200 dark:border-slate-700">
          <button
            onClick={onClose}
            disabled={isExporting}
            className="px-3 py-1.5 rounded border border-slate-300 dark:border-slate-600 text-slate-700 dark:text-slate-200 hover:bg-slate-100 dark:hover:bg-slate-700 transition"
          >
            取消
          </button>
          <button
            onClick={handleExport}
            disabled={isExporting}
            className="px-3.5 py-1.5 rounded bg-blue-600 text-white hover:bg-blue-700 transition flex items-center gap-1.5 font-medium"
          >
            {isExporting ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Download className="w-3.5 h-3.5" />}
            <span>{isExporting ? '正在导出...' : '选择位置并保存'}</span>
          </button>
        </div>
      </div>
    </div>
  );
};
