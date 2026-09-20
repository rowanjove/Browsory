import React, { useState, useRef, useEffect } from 'react';
import {
  Search,
  RefreshCw,
  SlidersHorizontal,
  Calendar,
  Trash2,
  Loader2,
  Download,
  ChevronLeft,
  ChevronRight,
  ChevronDown,
  Star,
  HelpCircle,
  X,
  ArrowUpDown,
  Bookmark,
  FolderPlus,
  Check,
} from 'lucide-react';
import { useHistoryStore, TimeRangeKey } from '../../stores/useHistoryStore';
import { useAppStore } from '../../stores/useAppStore';
import { tauriApi } from '../../services/tauri';
import { FilterAndColumnModal } from '../DataTable/FilterAndColumnModal';
import { ExportModal } from '../DataTable/ExportModal';

export const Topbar: React.FC = () => {
  const {
    search,
    setSearch,
    timeRange,
    setTimeRange,
    customStartTime,
    customEndTime,
    setCustomTimeRange,
    selectedDate,
    jumpToDate,
    goToPreviousDay,
    goToNextDay,
    onlyFavorites,
    toggleOnlyFavorites,
    selectedBrowsers,
    selectedProfiles,
    sortBy,
    setSortBy,
    smartCollections,
    activeCollectionId,
    loadSmartCollections,
    saveCurrentAsCollection,
    deleteCollection,
    applyCollection,
    selectedIds,
    deleteSelected,
    fetchHistory,
    isLoading,
  } = useHistoryStore();

  const { openRunningModal, showToast, t } = useAppStore();
  const [showFilterModal, setShowFilterModal] = useState(false);
  const [showExportModal, setShowExportModal] = useState(false);
  const [showSyntaxHelp, setShowSyntaxHelp] = useState(false);
  const [showCalendarPopover, setShowCalendarPopover] = useState(false);
  const [searchInput, setSearchInput] = useState(search);
  const [isSyncing, setIsSyncing] = useState(false);
  const [availableProfiles, setAvailableProfiles] = useState<string[]>([]);
  const [showSaveColModal, setShowSaveColModal] = useState(false);
  const [newColName, setNewColName] = useState('');
  const [showColListDropdown, setShowColListDropdown] = useState(false);

  // Custom date range inputs
  const [customStartInput, setCustomStartInput] = useState(() => {
    if (customStartTime) {
      const d = new Date(customStartTime);
      return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
    }
    const today = new Date();
    return `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, '0')}-${String(today.getDate()).padStart(2, '0')}`;
  });

  const [customEndInput, setCustomEndInput] = useState(() => {
    if (customEndTime) {
      const d = new Date(customEndTime);
      return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
    }
    const today = new Date();
    return `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, '0')}-${String(today.getDate()).padStart(2, '0')}`;
  });

  const syntaxRef = useRef<HTMLDivElement>(null);
  const colDropdownRef = useRef<HTMLDivElement>(null);
  const calendarPopoverRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    loadSmartCollections();
    tauriApi.scanBrowsers().then((profiles) => {
      const names = Array.from(new Set(profiles.map((p) => p.profile_name))).filter(Boolean);
      setAvailableProfiles(names);
    }).catch(console.error);
  }, []);

  useEffect(() => {
    setSearchInput(search);
  }, [search]);

  // Close syntax helper on outside click
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (syntaxRef.current && !syntaxRef.current.contains(e.target as Node)) {
        setShowSyntaxHelp(false);
      }
    };
    if (showSyntaxHelp) {
      document.addEventListener('mousedown', handleClickOutside);
    }
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, [showSyntaxHelp]);

  // Close collection dropdown on outside click
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (colDropdownRef.current && !colDropdownRef.current.contains(e.target as Node)) {
        setShowColListDropdown(false);
      }
    };
    if (showColListDropdown) {
      document.addEventListener('mousedown', handleClickOutside);
    }
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, [showColListDropdown]);

  // Close calendar popover on outside click
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (calendarPopoverRef.current && !calendarPopoverRef.current.contains(e.target as Node)) {
        setShowCalendarPopover(false);
      }
    };
    if (showCalendarPopover) {
      document.addEventListener('mousedown', handleClickOutside);
    }
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, [showCalendarPopover]);

  const handleSaveCollection = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!newColName.trim()) return;
    try {
      await saveCurrentAsCollection(newColName.trim());
      showToast(`智能集合 "${newColName.trim()}" 已创建`, 'success');
      setNewColName('');
      setShowSaveColModal(false);
    } catch (err: any) {
      showToast(err?.message || '保存集合失败', 'error');
    }
  };

  const handleSearchSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    setSearch(searchInput);
  };

  const handleInsertSyntax = (snippet: string) => {
    setSearchInput((prev) => (prev ? `${prev.trim()} ${snippet}` : snippet));
    setShowSyntaxHelp(false);
  };

  const handleApplyCustomRange = () => {
    if (!customStartInput || !customEndInput) return;
    const start = new Date(`${customStartInput}T00:00:00`).getTime();
    const end = new Date(`${customEndInput}T23:59:59.999`).getTime();
    setCustomTimeRange(start, end);
    setShowCalendarPopover(false);
  };

  const handleQuickSync = async () => {
    setIsSyncing(true);
    try {
      const isChromeRunning = await tauriApi.checkBrowserRunning('chrome');
      const isEdgeRunning = await tauriApi.checkBrowserRunning('edge');

      if (isChromeRunning) {
        openRunningModal('Chrome');
        setIsSyncing(false);
        return;
      }
      if (isEdgeRunning) {
        openRunningModal('Edge');
        setIsSyncing(false);
        return;
      }

      const results = await tauriApi.syncAll();
      const inserted = results.reduce((acc, r) => acc + r.inserted_count, 0);
      const duplicate = results.reduce((acc, r) => acc + r.duplicate_count, 0);

      showToast(t('sources.syncSuccess', { inserted, duplicate }), 'success');
      fetchHistory(true);
    } catch (err: any) {
      showToast(err?.message || '同步失败', 'error');
    } finally {
      setIsSyncing(false);
    }
  };

  const handleDelete = async () => {
    if (confirm(t('history.deleteConfirmDesc'))) {
      const count = await deleteSelected();
      showToast(`已从本地归档删除 ${count} 条记录`, 'info');
    }
  };

  const hasActiveFilters = selectedBrowsers.length > 0 || selectedProfiles.length > 0;

  return (
    <header className="border-b border-slate-200 dark:border-slate-800 bg-white dark:bg-slate-900 flex flex-col shrink-0 text-xs select-none relative z-20">
      {/* Row 1: 工具与维度控制栏 */}
      <div className="h-10 px-3 flex items-center justify-between gap-2">
        {/* Left Side: 日期步进器 + 收藏 + 智能集合 */}
        <div className="flex items-center gap-2">
          {/* 日期控制器：独立日历图标 + 前后步进 + 合并时间范围 */}
          <div className="flex items-center gap-1 bg-slate-50 dark:bg-slate-800/80 border border-slate-200 dark:border-slate-700 rounded-md p-0.5">
            {/* 独立日历图标与弹窗容器 */}
            <div className="relative" ref={calendarPopoverRef}>
              <button
                type="button"
                onClick={() => setShowCalendarPopover((prev) => !prev)}
                className={`p-1 rounded transition cursor-pointer ${
                  showCalendarPopover || selectedDate || timeRange === 'custom'
                    ? 'bg-blue-100 dark:bg-blue-900/60 text-blue-600 dark:text-blue-300'
                    : 'text-slate-600 dark:text-slate-300 hover:bg-slate-200 dark:hover:bg-slate-700'
                }`}
                title="打开日历与日期范围选择"
              >
                <Calendar className="w-3.5 h-3.5" />
              </button>

              {/* 日历与自定义范围 Popover */}
              {showCalendarPopover && (
                <div className="absolute left-0 top-8 w-72 bg-white dark:bg-slate-800 border border-slate-200 dark:border-slate-700 shadow-2xl rounded-xl p-3.5 z-50 flex flex-col gap-3 text-xs">
                  {/* 1. 指定单日跳转 */}
                  <div className="flex flex-col gap-1.5">
                    <span className="font-semibold text-slate-700 dark:text-slate-200">
                      跳转到指定日期
                    </span>
                    <div className="flex items-center gap-1.5">
                      <input
                        type="date"
                        value={selectedDate || ''}
                        onChange={(e) => {
                          if (e.target.value) {
                            jumpToDate(e.target.value);
                            setShowCalendarPopover(false);
                          }
                        }}
                        className="flex-1 px-2 py-1 rounded-md border border-slate-200 dark:border-slate-700 bg-slate-50 dark:bg-slate-900 text-slate-800 dark:text-slate-200 font-mono text-xs focus:outline-none focus:border-blue-500 cursor-pointer"
                      />
                      {selectedDate && (
                        <button
                          type="button"
                          onClick={() => {
                            jumpToDate(null);
                            setShowCalendarPopover(false);
                          }}
                          className="px-2 py-1 rounded border border-slate-200 dark:border-slate-700 text-slate-500 hover:text-slate-700 text-[11px]"
                        >
                          清除
                        </button>
                      )}
                    </div>
                  </div>

                  {/* 2. 从 YYYY-MM-DD 到 YYYY-MM-DD 筛选 */}
                  <div className="flex flex-col gap-1.5 pt-2.5 border-t border-slate-100 dark:border-slate-700">
                    <span className="font-semibold text-slate-700 dark:text-slate-200">
                      自定义时间范围
                    </span>
                    <div className="flex flex-col gap-1.5">
                      <div className="flex items-center gap-1.5">
                        <span className="text-slate-400 text-[11px] w-5">从</span>
                        <input
                          type="date"
                          value={customStartInput}
                          onChange={(e) => setCustomStartInput(e.target.value)}
                          className="flex-1 px-2 py-1 rounded-md border border-slate-200 dark:border-slate-700 bg-slate-50 dark:bg-slate-900 text-slate-800 dark:text-slate-200 font-mono text-xs focus:outline-none focus:border-blue-500 cursor-pointer"
                        />
                      </div>
                      <div className="flex items-center gap-1.5">
                        <span className="text-slate-400 text-[11px] w-5">到</span>
                        <input
                          type="date"
                          value={customEndInput}
                          onChange={(e) => setCustomEndInput(e.target.value)}
                          className="flex-1 px-2 py-1 rounded-md border border-slate-200 dark:border-slate-700 bg-slate-50 dark:bg-slate-900 text-slate-800 dark:text-slate-200 font-mono text-xs focus:outline-none focus:border-blue-500 cursor-pointer"
                        />
                      </div>
                      <button
                        type="button"
                        onClick={handleApplyCustomRange}
                        className="w-full mt-1 py-1 rounded-md bg-blue-600 hover:bg-blue-700 text-white font-medium text-xs transition cursor-pointer shadow-xs"
                      >
                        应用时间范围
                      </button>
                    </div>
                  </div>
                </div>
              )}
            </div>

            {/* 前一天 */}
            <button
              type="button"
              onClick={goToPreviousDay}
              title="前一天"
              className="p-1 hover:bg-slate-200 dark:hover:bg-slate-700 rounded text-slate-600 dark:text-slate-300 transition"
            >
              <ChevronLeft className="w-3.5 h-3.5" />
            </button>

            {/* 合并时间范围与下拉尖括号按钮 */}
            {selectedDate ? (
              <div className="flex items-center gap-1 px-1.5">
                <span className="font-mono text-xs text-blue-600 dark:text-blue-400 font-semibold">
                  {selectedDate}
                </span>
                <button
                  type="button"
                  onClick={() => jumpToDate(null)}
                  className="p-0.5 hover:bg-slate-200 dark:hover:bg-slate-700 rounded text-slate-400 hover:text-slate-600"
                  title="清除指定日，恢复时间范围"
                >
                  <X className="w-3 h-3" />
                </button>
              </div>
            ) : timeRange === 'custom' ? (
              <button
                type="button"
                onClick={() => setShowCalendarPopover(true)}
                className="flex items-center gap-1 px-1.5 py-0.5 text-xs text-blue-600 dark:text-blue-400 font-medium hover:bg-blue-50 dark:hover:bg-blue-950/40 rounded transition"
                title="点击修改自定义范围"
              >
                <span>自定义范围</span>
                <ChevronDown className="w-3 h-3 text-slate-400" />
              </button>
            ) : (
              <div className="relative flex items-center">
                <select
                  value={timeRange}
                  onChange={(e) => setTimeRange(e.target.value as TimeRangeKey)}
                  className="appearance-none bg-transparent text-slate-700 dark:text-slate-200 font-medium pl-1.5 pr-5 py-0.5 focus:outline-none cursor-pointer text-xs"
                  title="选择时间范围"
                >
                  <option value="all">{t('history.timeRange.all')}</option>
                  <option value="today">{t('history.timeRange.today')}</option>
                  <option value="yesterday">{t('history.timeRange.yesterday')}</option>
                  <option value="last7Days">{t('history.timeRange.last7Days')}</option>
                  <option value="last30Days">{t('history.timeRange.last30Days')}</option>
                  <option value="last90Days">近 90 天</option>
                  <option value="thisYear">{t('history.timeRange.thisYear')}</option>
                </select>
                <ChevronDown className="w-3 h-3 text-slate-400 absolute right-1 pointer-events-none" />
              </div>
            )}

            {/* 后一天 */}
            <button
              type="button"
              onClick={goToNextDay}
              title="后一天"
              className="p-1 hover:bg-slate-200 dark:hover:bg-slate-700 rounded text-slate-600 dark:text-slate-300 transition"
            >
              <ChevronRight className="w-3.5 h-3.5" />
            </button>
          </div>

          {/* 收藏过滤按钮 */}
          <button
            type="button"
            onClick={toggleOnlyFavorites}
            className={`flex items-center gap-1 px-2.5 py-1 rounded-md border text-xs transition whitespace-nowrap cursor-pointer ${
              onlyFavorites
                ? 'bg-amber-50 dark:bg-amber-950/60 border-amber-300 dark:border-amber-700 text-amber-700 dark:text-amber-300 font-medium shadow-xs'
                : 'border-slate-200 dark:border-slate-700 bg-slate-50 dark:bg-slate-800 text-slate-600 dark:text-slate-400 hover:bg-slate-100 dark:hover:bg-slate-700'
            }`}
            title="仅显示收藏网址"
          >
            <Star
              className={`w-3.5 h-3.5 ${
                onlyFavorites ? 'fill-amber-400 text-amber-500' : 'text-slate-400'
              }`}
            />
            <span>收藏</span>
          </button>

          {/* 智能集合下拉按钮 */}
          <div className="relative" ref={colDropdownRef}>
            <button
              type="button"
              onClick={() => setShowColListDropdown((prev) => !prev)}
              className={`flex items-center gap-1 px-2.5 py-1 rounded-md border text-xs transition whitespace-nowrap cursor-pointer ${
                activeCollectionId
                  ? 'bg-indigo-50 dark:bg-indigo-950/60 border-indigo-300 dark:border-indigo-700 text-indigo-700 dark:text-indigo-300 font-medium shadow-xs'
                  : 'border-slate-200 dark:border-slate-700 bg-slate-50 dark:bg-slate-800 text-slate-600 dark:text-slate-400 hover:bg-slate-100 dark:hover:bg-slate-700'
              }`}
              title="智能集合"
            >
              <Bookmark
                className={`w-3.5 h-3.5 ${
                  activeCollectionId ? 'fill-indigo-500 text-indigo-600' : 'text-slate-400'
                }`}
              />
              <span className="max-w-[80px] truncate">
                {activeCollectionId
                  ? smartCollections.find((c) => c.id === activeCollectionId)?.name || '集合'
                  : '智能集合'}
              </span>
              <ChevronDown className="w-3 h-3 text-slate-400" />
            </button>

            {/* 智能集合弹出列表 */}
            {showColListDropdown && (
              <div className="absolute left-0 top-8 w-64 bg-white dark:bg-slate-800 border border-slate-200 dark:border-slate-700 shadow-xl rounded-xl p-2 z-50 text-xs flex flex-col gap-1.5">
                <div className="flex items-center justify-between pb-1.5 border-b border-slate-100 dark:border-slate-700 px-1">
                  <span className="font-semibold text-slate-700 dark:text-slate-200">
                    智能搜索集合
                  </span>
                  <button
                    type="button"
                    onClick={() => {
                      setShowColListDropdown(false);
                      setShowSaveColModal(true);
                    }}
                    className="flex items-center gap-1 text-[11px] text-blue-600 dark:text-blue-400 hover:underline cursor-pointer"
                    title="将当前检索条件存为新集合"
                  >
                    <FolderPlus className="w-3 h-3" />
                    <span>保存当前</span>
                  </button>
                </div>

                {smartCollections.length === 0 ? (
                  <div className="text-slate-400 text-center py-3 text-[11px]">
                    暂无已保存的智能集合
                  </div>
                ) : (
                  <div className="flex flex-col gap-1 max-h-48 overflow-y-auto">
                    {smartCollections.map((col) => {
                      const isActive = activeCollectionId === col.id;
                      return (
                        <div
                          key={col.id}
                          className={`flex items-center justify-between px-2 py-1.5 rounded-lg transition cursor-pointer group ${
                            isActive
                              ? 'bg-indigo-50 dark:bg-indigo-950/50 text-indigo-700 dark:text-indigo-300 font-medium'
                              : 'hover:bg-slate-100 dark:hover:bg-slate-700/50 text-slate-700 dark:text-slate-200'
                          }`}
                          onClick={() => {
                            if (isActive) {
                              applyCollection(null);
                            } else {
                              applyCollection(col);
                            }
                            setShowColListDropdown(false);
                          }}
                        >
                          <div className="flex items-center gap-1.5 truncate">
                            {isActive && <Check className="w-3 h-3 text-indigo-600 shrink-0" />}
                            <span className="truncate">{col.name}</span>
                          </div>
                          <button
                            type="button"
                            onClick={(e) => {
                              e.stopPropagation();
                              if (confirm(`确定删除智能集合 "${col.name}" 吗？`)) {
                                deleteCollection(col.id);
                              }
                            }}
                            className="opacity-0 group-hover:opacity-100 p-0.5 hover:text-rose-500 transition text-slate-400"
                            title="删除集合"
                          >
                            <X className="w-3 h-3" />
                          </button>
                        </div>
                      );
                    })}
                  </div>
                )}
              </div>
            )}
          </div>
        </div>

        {/* Right Side: 筛选与列设置合并按钮 + 排序 + 批量操作 + 导出 + 同步 */}
        <div className="flex items-center gap-1.5">
          {/* 合并后的多维筛选与列设置按钮 */}
          <button
            type="button"
            onClick={() => setShowFilterModal(true)}
            className={`flex items-center gap-1.5 px-2.5 py-1 rounded-md border text-xs transition whitespace-nowrap cursor-pointer ${
              hasActiveFilters
                ? 'bg-blue-50 dark:bg-blue-950/60 border-blue-300 dark:border-blue-700 text-blue-600 dark:text-blue-300 font-medium shadow-xs'
                : 'border-slate-200 dark:border-slate-700 bg-slate-50 dark:bg-slate-800 text-slate-700 dark:text-slate-200 hover:bg-slate-100 dark:hover:bg-slate-700'
            }`}
            title="打开来源筛选与列设置"
          >
            <SlidersHorizontal className="w-3.5 h-3.5 text-blue-500" />
            <span>筛选与视图</span>
            {hasActiveFilters && (
              <span className="w-1.5 h-1.5 rounded-full bg-blue-500" />
            )}
          </button>

          {/* 排序方式选择 */}
          <div className="flex items-center gap-1 border border-slate-200 dark:border-slate-700 rounded-md px-1.5 py-1 bg-slate-50 dark:bg-slate-800 whitespace-nowrap">
            <ArrowUpDown className="w-3 h-3 text-slate-400 shrink-0" />
            <select
              value={sortBy}
              onChange={(e) => setSortBy(e.target.value as any)}
              className="bg-transparent text-slate-700 dark:text-slate-200 focus:outline-none cursor-pointer text-xs"
              title="排序方式"
            >
              <option value="time_desc">时间最新</option>
              <option value="time_asc">时间最早</option>
              <option value="visit_count">高频访问</option>
              <option value="duration">停留最长</option>
            </select>
          </div>

          {/* 批量删除按钮（有选中时出现） */}
          {selectedIds.size > 0 && (
            <button
              type="button"
              onClick={handleDelete}
              className="flex items-center gap-1 px-2.5 py-1 rounded-md bg-rose-50 text-rose-600 dark:bg-rose-950/60 dark:text-rose-400 border border-rose-200 dark:border-rose-900 hover:bg-rose-100 transition whitespace-nowrap shadow-xs font-medium cursor-pointer"
              title="删除选中的记录"
            >
              <Trash2 className="w-3.5 h-3.5 shrink-0" />
              <span>已选 {selectedIds.size} 项</span>
            </button>
          )}

          {/* 导出按钮 */}
          <button
            type="button"
            onClick={() => setShowExportModal(true)}
            className="flex items-center gap-1 px-2.5 py-1 rounded-md border border-slate-200 dark:border-slate-700 hover:bg-slate-100 dark:hover:bg-slate-800 text-slate-700 dark:text-slate-200 transition whitespace-nowrap cursor-pointer"
            title={t('history.export')}
          >
            <Download className="w-3.5 h-3.5 text-slate-500" />
            <span>{t('history.export')}</span>
          </button>

          {/* 立即同步主按钮 */}
          <button
            type="button"
            onClick={handleQuickSync}
            disabled={isSyncing || isLoading}
            className="flex items-center gap-1.5 px-3 py-1 rounded-md bg-blue-600 text-white hover:bg-blue-700 transition disabled:opacity-50 whitespace-nowrap shadow-xs font-medium cursor-pointer"
          >
            {isSyncing ? (
              <Loader2 className="w-3.5 h-3.5 animate-spin" />
            ) : (
              <RefreshCw className="w-3.5 h-3.5" />
            )}
            <span>{t('history.sync')}</span>
          </button>
        </div>
      </div>

      {/* Row 2: 宽幅独立搜索输入栏与语法助手 */}
      <div className="px-3 py-1.5 border-t border-slate-100 dark:border-slate-800/80 bg-slate-50/40 dark:bg-slate-900/40 flex items-center relative" ref={syntaxRef}>
        <form onSubmit={handleSearchSubmit} className="relative flex-1 flex items-center">
          <Search className="w-3.5 h-3.5 text-slate-400 absolute left-2.5 top-1/2 -translate-y-1/2 pointer-events-none" />
          <input
            type="text"
            value={searchInput}
            onChange={(e) => setSearchInput(e.target.value)}
            onBlur={() => {
              if (searchInput !== search) {
                setSearch(searchInput);
              }
            }}
            placeholder="搜索历史记录，支持关键词检索或输入高级语法如 site:github.com、browser:chrome 等..."
            className="w-full pl-8 pr-16 py-1 rounded-lg border border-slate-200 dark:border-slate-700/80 bg-white dark:bg-slate-800 text-slate-800 dark:text-slate-100 placeholder-slate-400 focus:outline-none focus:border-blue-500 transition text-xs shadow-2xs"
          />

          <div className="absolute right-2 flex items-center gap-1">
            {searchInput && (
              <button
                type="button"
                onClick={() => {
                  setSearchInput('');
                  setSearch('');
                }}
                className="p-1 text-slate-400 hover:text-slate-600 dark:hover:text-slate-200 rounded cursor-pointer"
                title="清空搜索"
              >
                <X className="w-3 h-3" />
              </button>
            )}

            <button
              type="button"
              onClick={() => setShowSyntaxHelp((prev) => !prev)}
              className={`p-1 rounded transition cursor-pointer ${
                showSyntaxHelp
                  ? 'text-blue-600 dark:text-blue-400 bg-blue-50 dark:bg-blue-950/60'
                  : 'text-slate-400 hover:text-slate-600 dark:hover:text-slate-300'
              }`}
              title="搜索高级语法助手"
            >
              <HelpCircle className="w-3.5 h-3.5" />
            </button>
          </div>
        </form>

        {/* 语法助手下拉卡片 (修复后无父级裁切，定位清晰) */}
        {showSyntaxHelp && (
          <div className="absolute top-10 right-3 w-88 bg-white dark:bg-slate-800 border border-slate-200 dark:border-slate-700 shadow-2xl rounded-xl p-3.5 z-50 text-[11px] flex flex-col gap-2 animate-in fade-in duration-100">
            <div className="font-semibold text-slate-700 dark:text-slate-200 flex items-center justify-between pb-1 border-b border-slate-100 dark:border-slate-700">
              <div className="flex items-center gap-1.5">
                <HelpCircle className="w-3.5 h-3.5 text-blue-500" />
                <span>高级搜索语法助手</span>
              </div>
              <span className="text-[10px] text-slate-400 font-normal">点击直接填入</span>
            </div>

            <div className="grid grid-cols-1 gap-1.5">
              <button
                type="button"
                onClick={() => handleInsertSyntax('site:github.com')}
                className="flex items-center justify-between px-2.5 py-1.5 rounded-lg bg-slate-50 dark:bg-slate-700/50 hover:bg-blue-50 dark:hover:bg-blue-950/60 text-left group transition cursor-pointer"
              >
                <code className="text-blue-600 dark:text-blue-400 font-mono font-medium">site:github.com</code>
                <span className="text-slate-500 group-hover:text-blue-600">限定网站域名</span>
              </button>
              <button
                type="button"
                onClick={() => handleInsertSyntax('browser:chrome')}
                className="flex items-center justify-between px-2.5 py-1.5 rounded-lg bg-slate-50 dark:bg-slate-700/50 hover:bg-blue-50 dark:hover:bg-blue-950/60 text-left group transition cursor-pointer"
              >
                <code className="text-blue-600 dark:text-blue-400 font-mono font-medium">browser:chrome</code>
                <span className="text-slate-500 group-hover:text-blue-600">限定浏览器</span>
              </button>
              <button
                type="button"
                onClick={() => handleInsertSyntax('profile:Default')}
                className="flex items-center justify-between px-2.5 py-1.5 rounded-lg bg-slate-50 dark:bg-slate-700/50 hover:bg-blue-50 dark:hover:bg-blue-950/60 text-left group transition cursor-pointer"
              >
                <code className="text-blue-600 dark:text-blue-400 font-mono font-medium">profile:Default</code>
                <span className="text-slate-500 group-hover:text-blue-600">限定配置文件</span>
              </button>
              <button
                type="button"
                onClick={() => handleInsertSyntax('title:React')}
                className="flex items-center justify-between px-2.5 py-1.5 rounded-lg bg-slate-50 dark:bg-slate-700/50 hover:bg-blue-50 dark:hover:bg-blue-950/60 text-left group transition cursor-pointer"
              >
                <code className="text-blue-600 dark:text-blue-400 font-mono font-medium">title:React</code>
                <span className="text-slate-500 group-hover:text-blue-600">限定标题关键词</span>
              </button>
              <button
                type="button"
                onClick={() => handleInsertSyntax('after:2026-01-01')}
                className="flex items-center justify-between px-2.5 py-1.5 rounded-lg bg-slate-50 dark:bg-slate-700/50 hover:bg-blue-50 dark:hover:bg-blue-950/60 text-left group transition cursor-pointer"
              >
                <code className="text-blue-600 dark:text-blue-400 font-mono font-medium">after:YYYY-MM-DD</code>
                <span className="text-slate-500 group-hover:text-blue-600">指定日期之后</span>
              </button>
              <button
                type="button"
                onClick={() => handleInsertSyntax('before:2026-12-31')}
                className="flex items-center justify-between px-2.5 py-1.5 rounded-lg bg-slate-50 dark:bg-slate-700/50 hover:bg-blue-50 dark:hover:bg-blue-950/60 text-left group transition cursor-pointer"
              >
                <code className="text-blue-600 dark:text-blue-400 font-mono font-medium">before:YYYY-MM-DD</code>
                <span className="text-slate-500 group-hover:text-blue-600">指定日期之前</span>
              </button>
            </div>
          </div>
        )}
      </div>

      {/* Save Collection Modal */}
      {showSaveColModal && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-xs p-4">
          <div className="bg-white dark:bg-slate-800 border border-slate-200 dark:border-slate-700 rounded-xl shadow-2xl w-full max-w-sm p-5 flex flex-col gap-4">
            <div className="flex items-center justify-between">
              <h3 className="font-semibold text-slate-800 dark:text-slate-100 text-sm">保存为智能集合</h3>
              <button
                type="button"
                onClick={() => setShowSaveColModal(false)}
                className="p-1 text-slate-400 hover:text-slate-600 rounded cursor-pointer"
              >
                <X className="w-4 h-4" />
              </button>
            </div>
            <form onSubmit={handleSaveCollection} className="flex flex-col gap-3">
              <div className="flex flex-col gap-1">
                <label className="text-slate-500 dark:text-slate-400 text-xs">集合名称</label>
                <input
                  type="text"
                  required
                  autoFocus
                  placeholder="例如：GitHub 技术调研 / 常用开发文档"
                  value={newColName}
                  onChange={(e) => setNewColName(e.target.value)}
                  className="px-3 py-1.5 rounded-lg border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-900 text-slate-800 dark:text-slate-100 focus:outline-none focus:border-blue-500 text-xs"
                />
              </div>
              <div className="p-2.5 rounded-lg bg-slate-50 dark:bg-slate-900/60 border border-slate-100 dark:border-slate-800 text-[11px] text-slate-500 flex flex-col gap-1">
                <div>搜索词: <span className="text-blue-600 font-mono">{search || '(全部)'}</span></div>
                <div>时间范围: <span className="font-medium text-slate-700 dark:text-slate-300">{timeRange}</span></div>
                <div>排序方式: <span className="font-medium text-slate-700 dark:text-slate-300">{sortBy}</span></div>
              </div>
              <div className="flex justify-end gap-2 pt-2">
                <button
                  type="button"
                  onClick={() => setShowSaveColModal(false)}
                  className="px-3 py-1.5 rounded-lg border border-slate-300 dark:border-slate-700 text-slate-600 dark:text-slate-300 hover:bg-slate-100 dark:hover:bg-slate-700 cursor-pointer"
                >
                  取消
                </button>
                <button
                  type="submit"
                  className="px-3.5 py-1.5 rounded-lg bg-blue-600 text-white hover:bg-blue-700 transition font-medium cursor-pointer"
                >
                  保存集合
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      <FilterAndColumnModal
        isOpen={showFilterModal}
        onClose={() => setShowFilterModal(false)}
        availableProfiles={availableProfiles}
      />
      <ExportModal isOpen={showExportModal} onClose={() => setShowExportModal(false)} />
    </header>
  );
};
