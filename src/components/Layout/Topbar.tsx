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
import { describeSyncResults } from '../../services/syncFeedback';
import { useSecurityStore } from '../../stores/useSecurityStore';
import { useDialogFocus } from '../../hooks/useDialogFocus';
import { FilterAndColumnModal } from '../DataTable/FilterAndColumnModal';
import { ExportModal } from '../DataTable/ExportModal';

export const Topbar: React.FC = () => {
  const {
    search,
    items,
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
    availableProfiles,
    availableBrowserIds,
    loadSmartCollections,
    loadFilterOptions,
    saveCurrentAsCollection,
    deleteCollection,
    applyCollection,
    selectedIds,
    deleteSelected,
    fetchHistory,
    isLoading,
    isDeleting,
  } = useHistoryStore();

  const { openRunningModal, showToast, t, language } = useAppStore();
  const ui = (zh: string, en: string) => (language === 'en-US' ? en : zh);
  const [showFilterModal, setShowFilterModal] = useState(false);
  const [showExportModal, setShowExportModal] = useState(false);
  const [showSyntaxHelp, setShowSyntaxHelp] = useState(false);
  const [showCalendarPopover, setShowCalendarPopover] = useState(false);
  const [searchInput, setSearchInput] = useState(search);
  const [isSyncing, setIsSyncing] = useState(false);
  const [showSaveColModal, setShowSaveColModal] = useState(false);
  const [newColName, setNewColName] = useState('');
  const [showColListDropdown, setShowColListDropdown] = useState(false);
  const [confirmDeleteColId, setConfirmDeleteColId] = useState<number | null>(null);
  const [confirmBatchDelete, setConfirmBatchDelete] = useState(false);

  const saveColNameInputRef = useRef<HTMLInputElement>(null);
  const saveColDialogRef = useDialogFocus<HTMLDivElement>({
    isOpen: showSaveColModal,
    onClose: () => setShowSaveColModal(false),
    initialFocusRef: saveColNameInputRef,
  });

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
    loadFilterOptions();
  }, [loadFilterOptions, loadSmartCollections]);

  useEffect(() => {
    setSearchInput(search);
  }, [search]);

  useEffect(() => {
    if (selectedIds.size === 0) {
      setConfirmBatchDelete(false);
    }
  }, [selectedIds.size]);

  // Close popovers on outside click or Escape
  useEffect(() => {
    if (!showSyntaxHelp && !showColListDropdown && !showCalendarPopover) return;
    const handleClickOutside = (e: MouseEvent) => {
      const target = e.target as Node;
      if (showSyntaxHelp && syntaxRef.current && !syntaxRef.current.contains(target)) {
        setShowSyntaxHelp(false);
      }
      if (showColListDropdown && colDropdownRef.current && !colDropdownRef.current.contains(target)) {
        setShowColListDropdown(false);
        setConfirmDeleteColId(null);
      }
      if (showCalendarPopover && calendarPopoverRef.current && !calendarPopoverRef.current.contains(target)) {
        setShowCalendarPopover(false);
      }
    };
    const handleEscape = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        setShowSyntaxHelp(false);
        setShowColListDropdown(false);
        setShowCalendarPopover(false);
        setConfirmDeleteColId(null);
      }
    };
    document.addEventListener('mousedown', handleClickOutside);
    document.addEventListener('keydown', handleEscape);
    return () => {
      document.removeEventListener('mousedown', handleClickOutside);
      document.removeEventListener('keydown', handleEscape);
    };
  }, [showSyntaxHelp, showColListDropdown, showCalendarPopover]);

  const handleSaveCollection = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!newColName.trim()) return;
    try {
      await saveCurrentAsCollection(newColName.trim());
      showToast(ui(`智能集合 "${newColName.trim()}" 已创建`, `Collection "${newColName.trim()}" created`), 'success');
      setNewColName('');
      setShowSaveColModal(false);
    } catch (err: any) {
      showToast(err?.message || ui('保存集合失败', 'Failed to save collection'), 'error');
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
    const startDate = new Date(`${customStartInput}T00:00:00`);
    const endDate = new Date(`${customEndInput}T00:00:00`);
    if (Number.isNaN(startDate.getTime()) || Number.isNaN(endDate.getTime())) {
      showToast(ui('请输入有效的日期范围', 'Enter a valid date range'), 'warning');
      return;
    }
    if (startDate.getTime() > endDate.getTime()) {
      showToast(ui('开始日期不能晚于结束日期', 'Start date cannot be after end date'), 'warning');
      return;
    }
    // Advance by a local calendar day so the inclusive end remains DST-safe.
    endDate.setDate(endDate.getDate() + 1);
    endDate.setMilliseconds(-1);
    setCustomTimeRange(startDate.getTime(), endDate.getTime());
    setShowCalendarPopover(false);
  };

  const handleQuickSync = async () => {
    setIsSyncing(true);
    try {
      const results = await tauriApi.syncAll();
      const feedback = describeSyncResults(results, useAppStore.getState().language);
      useSecurityStore.getState().recordSyncResults(results);

      showToast(feedback.message, feedback.type);
      fetchHistory(true);
    } catch (err: any) {
      const msg = typeof err === 'string' ? err : err?.message || ui('同步失败', 'Sync failed');
      if (/快照|高频写入|完整性校验/.test(msg)) {
        openRunningModal('Chrome');
      }
      showToast(msg, 'error');
    } finally {
      setIsSyncing(false);
    }
  };

  const handleConfirmDelete = async () => {
    if (isDeleting) return;
    setConfirmBatchDelete(false);
    try {
      const count = await deleteSelected();
      showToast(ui(`已从本地归档删除 ${count} 条记录`, `Removed ${count} record(s) from the local archive`), 'info');
    } catch (err: any) {
      showToast(err?.message || ui('删除历史记录失败', 'Failed to delete history records'), 'error');
    }
  };

  const hasActiveFilters = selectedBrowsers.length > 0 || selectedProfiles.length > 0;

  return (
    <header className="border-b border-slate-200 dark:border-slate-800 bg-white dark:bg-slate-900 flex flex-col shrink-0 text-xs select-none relative z-20">
      {/* Row 1: Tool and Dimension Control Bar */}
      <div className="h-10 px-3 flex items-center justify-between gap-2">
        {/* Left Side: Date Stepper + Favorites + Smart Collections */}
        <div className="flex items-center gap-2">
          {/* Date Controller */}
          <div className="flex items-center gap-1 bg-slate-50 dark:bg-slate-800/80 border border-slate-200 dark:border-slate-700 rounded-md p-0.5">
            {/* Calendar Icon and Popover */}
            <div className="relative" ref={calendarPopoverRef}>
              <button
                type="button"
                onClick={() => setShowCalendarPopover((prev) => !prev)}
                className={`p-1 rounded transition cursor-pointer ${
                  showCalendarPopover || selectedDate || timeRange === 'custom'
                    ? 'bg-blue-100 dark:bg-blue-900/60 text-blue-600 dark:text-blue-300'
                    : 'text-slate-600 dark:text-slate-300 hover:bg-slate-200 dark:hover:bg-slate-700'
                }`}
                aria-label={ui('打开日历与日期范围选择', 'Open calendar and date range picker')}
                title={ui('打开日历与日期范围选择', 'Open calendar and date range picker')}
              >
                <Calendar className="w-3.5 h-3.5" />
              </button>

              {/* Calendar and Custom Range Popover */}
              {showCalendarPopover && (
                <div className="absolute left-0 top-8 w-72 bg-white dark:bg-slate-800 border border-slate-200 dark:border-slate-700 shadow-2xl rounded-xl p-3.5 z-50 flex flex-col gap-3 text-xs">
                  {/* 1. Jump to specific date */}
                  <div className="flex flex-col gap-1.5">
                    <span className="font-semibold text-slate-700 dark:text-slate-200">
                      {ui('跳转到指定日期', 'Jump to date')}
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
                          className="px-2 py-1 rounded border border-slate-200 dark:border-slate-700 text-slate-500 hover:text-slate-700 text-[11px] cursor-pointer"
                        >
                          {ui('清除', 'Clear')}
                        </button>
                      )}
                    </div>
                  </div>

                  {/* 2. Custom range */}
                  <div className="flex flex-col gap-1.5 pt-2.5 border-t border-slate-100 dark:border-slate-700">
                    <span className="font-semibold text-slate-700 dark:text-slate-200">
                      {ui('自定义时间范围', 'Custom date range')}
                    </span>
                    <div className="flex flex-col gap-1.5">
                      <div className="flex items-center gap-1.5">
                        <span className="text-slate-400 text-[11px] w-7">{ui('从', 'From')}</span>
                        <input
                          type="date"
                          value={customStartInput}
                          onChange={(e) => setCustomStartInput(e.target.value)}
                          className="flex-1 px-2 py-1 rounded-md border border-slate-200 dark:border-slate-700 bg-slate-50 dark:bg-slate-900 text-slate-800 dark:text-slate-200 font-mono text-xs focus:outline-none focus:border-blue-500 cursor-pointer"
                        />
                      </div>
                      <div className="flex items-center gap-1.5">
                        <span className="text-slate-400 text-[11px] w-7">{ui('到', 'To')}</span>
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
                        className="w-full mt-1 py-1.5 rounded-md bg-blue-600 hover:bg-blue-700 text-white font-medium text-xs transition cursor-pointer shadow-xs"
                      >
                        {ui('应用时间范围', 'Apply range')}
                      </button>
                    </div>
                  </div>
                </div>
              )}
            </div>

            {/* Previous day */}
            <button
              type="button"
              onClick={goToPreviousDay}
              aria-label={ui('前一天', 'Previous day')}
              title={ui('前一天', 'Previous day')}
              className="p-1 hover:bg-slate-200 dark:hover:bg-slate-700 rounded text-slate-600 dark:text-slate-300 transition cursor-pointer"
            >
              <ChevronLeft className="w-3.5 h-3.5" />
            </button>

            {/* Range Display / Selector */}
            {selectedDate ? (
              <div className="flex items-center gap-1 px-1.5">
                <span className="font-mono text-xs text-blue-600 dark:text-blue-400 font-semibold">
                  {selectedDate}
                </span>
                <button
                  type="button"
                  onClick={() => jumpToDate(null)}
                  className="p-0.5 hover:bg-slate-200 dark:hover:bg-slate-700 rounded text-slate-400 hover:text-slate-600 cursor-pointer"
                  aria-label={ui('清除指定日，恢复时间范围', 'Clear date filter')}
                  title={ui('清除指定日，恢复时间范围', 'Clear date filter')}
                >
                  <X className="w-3 h-3" />
                </button>
              </div>
            ) : timeRange === 'custom' ? (
              <button
                type="button"
                onClick={() => setShowCalendarPopover(true)}
                className="flex items-center gap-1 px-1.5 py-0.5 text-xs text-blue-600 dark:text-blue-400 font-medium hover:bg-blue-50 dark:hover:bg-blue-950/40 rounded transition cursor-pointer"
                title={ui('点击修改自定义范围', 'Click to change custom range')}
              >
                <span>{ui('自定义范围', 'Custom')}</span>
                <ChevronDown className="w-3 h-3 text-slate-400" />
              </button>
            ) : (
              <div className="relative flex items-center">
                <select
                  value={timeRange}
                  onChange={(e) => setTimeRange(e.target.value as TimeRangeKey)}
                  aria-label={ui('选择时间范围', 'Select time range')}
                  className="appearance-none bg-transparent text-slate-700 dark:text-slate-200 font-medium pl-1.5 pr-5 py-0.5 focus:outline-none cursor-pointer text-xs"
                  title={ui('选择时间范围', 'Select time range')}
                >
                  <option value="all">{t('history.timeRange.all')}</option>
                  <option value="today">{t('history.timeRange.today')}</option>
                  <option value="yesterday">{t('history.timeRange.yesterday')}</option>
                  <option value="last7Days">{t('history.timeRange.last7Days')}</option>
                  <option value="last30Days">{t('history.timeRange.last30Days')}</option>
                  <option value="last90Days">{ui('近 90 天', 'Last 90 Days')}</option>
                  <option value="thisYear">{t('history.timeRange.thisYear')}</option>
                </select>
                <ChevronDown className="w-3 h-3 text-slate-400 absolute right-1 pointer-events-none" />
              </div>
            )}

            {/* Next day */}
            <button
              type="button"
              onClick={goToNextDay}
              aria-label={ui('后一天', 'Next day')}
              title={ui('后一天', 'Next day')}
              className="p-1 hover:bg-slate-200 dark:hover:bg-slate-700 rounded text-slate-600 dark:text-slate-300 transition cursor-pointer"
            >
              <ChevronRight className="w-3.5 h-3.5" />
            </button>
          </div>

          {/* Favorites toggle */}
          <button
            type="button"
            onClick={toggleOnlyFavorites}
            className={`flex items-center gap-1 px-2.5 py-1 rounded-md border text-xs transition whitespace-nowrap cursor-pointer ${
              onlyFavorites
                ? 'bg-amber-50 dark:bg-amber-950/60 border-amber-300 dark:border-amber-700 text-amber-700 dark:text-amber-300 font-medium shadow-xs'
                : 'border-slate-200 dark:border-slate-700 bg-slate-50 dark:bg-slate-800 text-slate-600 dark:text-slate-400 hover:bg-slate-100 dark:hover:bg-slate-700'
            }`}
            title={ui('仅显示收藏网址', 'Show favorites only')}
          >
            <Star
              className={`w-3.5 h-3.5 ${
                onlyFavorites ? 'fill-amber-400 text-amber-500' : 'text-slate-400'
              }`}
            />
            <span>{ui('收藏', 'Favorites')}</span>
          </button>

          {/* Smart Collections dropdown */}
          <div className="relative" ref={colDropdownRef}>
            <button
              type="button"
              onClick={() => setShowColListDropdown((prev) => !prev)}
              className={`flex items-center gap-1 px-2.5 py-1 rounded-md border text-xs transition whitespace-nowrap cursor-pointer ${
                activeCollectionId
                  ? 'bg-blue-50 dark:bg-blue-950/60 border-blue-300 dark:border-blue-700 text-blue-700 dark:text-blue-300 font-medium shadow-xs'
                  : 'border-slate-200 dark:border-slate-700 bg-slate-50 dark:bg-slate-800 text-slate-600 dark:text-slate-400 hover:bg-slate-100 dark:hover:bg-slate-700'
              }`}
              title={ui('智能集合', 'Saved filters')}
            >
              <Bookmark
                className={`w-3.5 h-3.5 ${
                  activeCollectionId ? 'fill-blue-500 text-blue-600' : 'text-slate-400'
                }`}
              />
              <span className="max-w-[90px] truncate">
                {activeCollectionId
                  ? smartCollections.find((c) => c.id === activeCollectionId)?.name || ui('集合', 'Collection')
                  : ui('智能集合', 'Collections')}
              </span>
              <ChevronDown className="w-3 h-3 text-slate-400" />
            </button>

            {showColListDropdown && (
              <div className="absolute left-0 top-8 w-72 bg-white dark:bg-slate-800 border border-slate-200 dark:border-slate-700 shadow-xl rounded-xl p-2 z-50 text-xs flex flex-col gap-1.5">
                <div className="flex items-center justify-between pb-1.5 border-b border-slate-100 dark:border-slate-700 px-1">
                  <span className="font-semibold text-slate-700 dark:text-slate-200">
                    {ui('智能搜索集合', 'Saved filter collections')}
                  </span>
                  <button
                    type="button"
                    onClick={() => {
                      setShowColListDropdown(false);
                      setShowSaveColModal(true);
                    }}
                    className="flex items-center gap-1 text-[11px] text-blue-600 dark:text-blue-400 hover:underline cursor-pointer"
                    title={ui('将当前检索条件存为新集合', 'Save current filters as collection')}
                  >
                    <FolderPlus className="w-3 h-3" />
                    <span>{ui('保存当前', 'Save current')}</span>
                  </button>
                </div>

                {smartCollections.length === 0 ? (
                  <div className="text-slate-400 text-center py-3 text-xs">
                    {ui('暂无已保存的智能集合', 'No saved collections yet')}
                  </div>
                ) : (
                  <div className="flex flex-col gap-1 max-h-48 overflow-y-auto">
                    {smartCollections.map((col) => {
                      const isActive = activeCollectionId === col.id;
                      const isConfirming = confirmDeleteColId === col.id;
                      return (
                        <div
                          key={col.id}
                          className={`flex items-center justify-between px-2 py-1.5 rounded-lg transition cursor-pointer group ${
                            isActive
                              ? 'bg-blue-50 dark:bg-blue-950/50 text-blue-700 dark:text-blue-300 font-medium'
                              : 'hover:bg-slate-100 dark:hover:bg-slate-700/50 text-slate-700 dark:text-slate-200'
                          }`}
                          onClick={() => {
                            if (isConfirming) return;
                            if (isActive) {
                              applyCollection(null);
                            } else {
                              applyCollection(col);
                            }
                            setShowColListDropdown(false);
                          }}
                        >
                          <div className="flex items-center gap-1.5 truncate">
                            {isActive && <Check className="w-3 h-3 text-blue-600 shrink-0" />}
                            <span className="truncate">{col.name}</span>
                          </div>
                          {isConfirming ? (
                            <div className="flex items-center gap-1 shrink-0" onClick={(e) => e.stopPropagation()}>
                              <button
                                type="button"
                                onClick={() => {
                                  deleteCollection(col.id);
                                  setConfirmDeleteColId(null);
                                }}
                                className="px-1.5 py-0.5 rounded bg-rose-600 text-white text-[11px] cursor-pointer"
                              >
                                {ui('删除', 'Delete')}
                              </button>
                              <button
                                type="button"
                                onClick={() => setConfirmDeleteColId(null)}
                                className="px-1.5 py-0.5 rounded border border-slate-300 dark:border-slate-600 text-[11px] cursor-pointer"
                              >
                                {t('export.cancel')}
                              </button>
                            </div>
                          ) : (
                            <button
                              type="button"
                              onClick={(e) => {
                                e.stopPropagation();
                                setConfirmDeleteColId(col.id);
                              }}
                              aria-label={ui(`删除集合 ${col.name}`, `Delete collection ${col.name}`)}
                              className="opacity-0 group-hover:opacity-100 focus:opacity-100 p-0.5 hover:text-rose-500 transition text-slate-400 cursor-pointer"
                              title={ui('删除集合', 'Delete collection')}
                            >
                              <X className="w-3 h-3" />
                            </button>
                          )}
                        </div>
                      );
                    })}
                  </div>
                )}
              </div>
            )}
          </div>
        </div>

        {/* Right Side: Filter/Columns + Sort + Batch Delete + Export + Sync */}
        <div className="flex items-center gap-1.5">
          <button
            type="button"
            onClick={() => setShowFilterModal(true)}
            className={`flex items-center gap-1.5 px-2.5 py-1 rounded-md border text-xs transition whitespace-nowrap cursor-pointer ${
              hasActiveFilters
                ? 'bg-blue-50 dark:bg-blue-950/60 border-blue-300 dark:border-blue-700 text-blue-600 dark:text-blue-300 font-medium shadow-xs'
                : 'border-slate-200 dark:border-slate-700 bg-slate-50 dark:bg-slate-800 text-slate-700 dark:text-slate-200 hover:bg-slate-100 dark:hover:bg-slate-700'
            }`}
            title={ui('打开来源筛选与列设置', 'Open source filters and column settings')}
          >
            <SlidersHorizontal className="w-3.5 h-3.5 text-blue-500" />
            <span>{ui('筛选与视图', 'Filters & view')}</span>
            {hasActiveFilters && (
              <span className="w-1.5 h-1.5 rounded-full bg-blue-500" />
            )}
          </button>

          {/* Sort */}
          <div className="flex items-center gap-1 border border-slate-200 dark:border-slate-700 rounded-md px-1.5 py-1 bg-slate-50 dark:bg-slate-800 whitespace-nowrap">
            <ArrowUpDown className="w-3 h-3 text-slate-400 shrink-0" />
            <select
              value={sortBy}
              onChange={(e) => setSortBy(e.target.value as any)}
              aria-label={ui('排序方式', 'Sort by')}
              className="bg-transparent text-slate-700 dark:text-slate-200 focus:outline-none cursor-pointer text-xs"
              title={ui('排序方式', 'Sort by')}
            >
              <option value="time_desc">{ui('时间最新', 'Newest first')}</option>
              <option value="time_asc">{ui('时间最早', 'Oldest first')}</option>
              <option value="visit_count">{ui('高频访问', 'Most visited')}</option>
              <option value="duration">{ui('停留最长', 'Longest duration')}</option>
            </select>
          </div>

          {/* Inline Batch Delete */}
          {selectedIds.size > 0 && (
            confirmBatchDelete ? (
              <div className="flex items-center gap-1 px-2 py-0.5 rounded-md bg-rose-50 dark:bg-rose-950/60 border border-rose-300 dark:border-rose-800 text-xs">
                <span className="text-rose-700 dark:text-rose-300">{ui(`删 ${selectedIds.size} 项?`, `Delete ${selectedIds.size}?`)}</span>
                <button
                  type="button"
                  onClick={handleConfirmDelete}
                  disabled={isDeleting}
                  className="px-1.5 py-0.5 rounded bg-rose-600 hover:bg-rose-700 text-white font-medium cursor-pointer"
                >
                  {ui('确认', 'Confirm')}
                </button>
                <button
                  type="button"
                  onClick={() => setConfirmBatchDelete(false)}
                  className="px-1.5 py-0.5 rounded border border-slate-300 dark:border-slate-700 text-slate-600 dark:text-slate-300 cursor-pointer"
                >
                  {t('export.cancel')}
                </button>
              </div>
            ) : (
              <button
                type="button"
                onClick={() => setConfirmBatchDelete(true)}
                disabled={isDeleting}
                aria-label={ui('删除选中的历史记录', 'Delete selected history records')}
                className="flex items-center gap-1 px-2.5 py-1 rounded-md bg-rose-50 text-rose-600 dark:bg-rose-950/60 dark:text-rose-400 border border-rose-200 dark:border-rose-900 hover:bg-rose-100 transition whitespace-nowrap shadow-xs font-medium cursor-pointer"
                title={t('history.deleteConfirmDesc')}
              >
                <Trash2 className="w-3.5 h-3.5 shrink-0" />
                <span>{t('history.selectedCount', { count: selectedIds.size })}</span>
              </button>
            )
          )}

          {/* Export */}
          <button
            type="button"
            onClick={() => setShowExportModal(true)}
            className="flex items-center gap-1 px-2.5 py-1 rounded-md border border-slate-200 dark:border-slate-700 hover:bg-slate-100 dark:hover:bg-slate-800 text-slate-700 dark:text-slate-200 transition whitespace-nowrap cursor-pointer"
            title={t('history.export')}
          >
            <Download className="w-3.5 h-3.5 text-slate-500" />
            <span>{t('history.export')}</span>
          </button>

          {/* Quick Sync */}
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

      {/* Row 2: Wide Search Bar + Syntax Helper */}
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
            placeholder={ui(
              '搜索历史记录，支持关键词检索或输入高级语法如 site:github.com、browser:chrome 等...',
              'Search history by keyword or filter syntax (site:github.com, browser:chrome, after:YYYY-MM-DD)...'
            )}
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
                aria-label={t('quickSearch.clear')}
                className="p-1 text-slate-400 hover:text-slate-600 dark:hover:text-slate-200 rounded cursor-pointer"
                title={t('quickSearch.clear')}
              >
                <X className="w-3 h-3" />
              </button>
            )}

            <button
              type="button"
              onClick={() => setShowSyntaxHelp((prev) => !prev)}
              aria-label={ui('搜索高级语法助手', 'Search syntax help')}
              aria-expanded={showSyntaxHelp}
              className={`p-1 rounded transition cursor-pointer ${
                showSyntaxHelp
                  ? 'text-blue-600 dark:text-blue-400 bg-blue-50 dark:bg-blue-950/60'
                  : 'text-slate-400 hover:text-slate-600 dark:hover:text-slate-300'
              }`}
              title={ui('搜索高级语法助手', 'Search syntax help')}
            >
              <HelpCircle className="w-3.5 h-3.5" />
            </button>
          </div>
        </form>

        {/* Syntax Helper Card */}
        {showSyntaxHelp && (
          <div className="absolute top-10 right-3 w-88 bg-white dark:bg-slate-800 border border-slate-200 dark:border-slate-700 shadow-2xl rounded-xl p-3.5 z-50 text-xs flex flex-col gap-2 animate-in fade-in duration-100">
            <div className="font-semibold text-slate-700 dark:text-slate-200 flex items-center justify-between pb-1 border-b border-slate-100 dark:border-slate-700">
              <div className="flex items-center gap-1.5">
                <HelpCircle className="w-3.5 h-3.5 text-blue-500" />
                <span>{ui('高级搜索语法助手', 'Search syntax helper')}</span>
              </div>
              <span className="text-[11px] text-slate-400 font-normal">{ui('点击直接填入', 'Click to insert')}</span>
            </div>

            <div className="grid grid-cols-1 gap-1.5">
              {[
                ['site:github.com', ui('限定网站域名', 'Filter domain')],
                ['browser:chrome', ui('限定浏览器', 'Filter browser')],
                ['profile:Default', ui('限定配置文件', 'Filter profile')],
                ['title:React', ui('限定标题关键词', 'Filter title keyword')],
                ['after:2026-01-01', ui('指定日期之后', 'After date (YYYY-MM-DD)')],
                ['before:2026-12-31', ui('指定日期之前', 'Before date (YYYY-MM-DD)')],
              ].map(([code, desc]) => (
                <button
                  key={code}
                  type="button"
                  onClick={() => handleInsertSyntax(code)}
                  className="flex items-center justify-between px-2.5 py-1.5 rounded-lg bg-slate-50 dark:bg-slate-700/50 hover:bg-blue-50 dark:hover:bg-blue-950/60 text-left group transition cursor-pointer"
                >
                  <code className="text-blue-600 dark:text-blue-400 font-mono font-medium">{code}</code>
                  <span className="text-slate-500 group-hover:text-blue-600">{desc}</span>
                </button>
              ))}
            </div>
          </div>
        )}
      </div>

      {/* Save Collection Modal */}
      {showSaveColModal && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-xs p-4"
          onClick={() => setShowSaveColModal(false)}
          role="presentation"
        >
          <div
            ref={saveColDialogRef}
            role="dialog"
            aria-modal="true"
            aria-labelledby="save-collection-title"
            tabIndex={-1}
            onClick={(e) => e.stopPropagation()}
            className="bg-white dark:bg-slate-800 border border-slate-200 dark:border-slate-700 rounded-xl shadow-2xl w-full max-w-sm p-5 flex flex-col gap-4"
          >
            <div className="flex items-center justify-between">
              <h3 id="save-collection-title" className="font-semibold text-slate-800 dark:text-slate-100 text-sm">
                {ui('保存为智能集合', 'Save filter collection')}
              </h3>
              <button
                type="button"
                onClick={() => setShowSaveColModal(false)}
                aria-label={ui('关闭保存集合窗口', 'Close save collection dialog')}
                className="p-1 text-slate-400 hover:text-slate-600 rounded cursor-pointer"
              >
                <X className="w-4 h-4" />
              </button>
            </div>
            <form onSubmit={handleSaveCollection} className="flex flex-col gap-3">
              <div className="flex flex-col gap-1">
                <label className="text-slate-500 dark:text-slate-400 text-xs">{ui('集合名称', 'Collection name')}</label>
                <input
                  ref={saveColNameInputRef}
                  type="text"
                  required
                  placeholder={ui('例如：GitHub 技术调研 / 常用开发文档', 'e.g. Rust research / Reading list')}
                  value={newColName}
                  onChange={(e) => setNewColName(e.target.value)}
                  className="px-3 py-1.5 rounded-lg border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-900 text-slate-800 dark:text-slate-100 focus:outline-none focus:border-blue-500 text-xs"
                />
              </div>
              <div className="p-2.5 rounded-lg bg-slate-50 dark:bg-slate-900/60 border border-slate-100 dark:border-slate-800 text-xs text-slate-500 flex flex-col gap-1">
                <div>{ui('搜索词:', 'Query:')} <span className="text-blue-600 font-mono">{search || ui('(全部)', '(All)')}</span></div>
                <div>{ui('时间范围:', 'Range:')} <span className="font-medium text-slate-700 dark:text-slate-300">{timeRange}</span></div>
                <div>{ui('排序方式:', 'Sort:')} <span className="font-medium text-slate-700 dark:text-slate-300">{sortBy}</span></div>
              </div>
              <div className="flex justify-end gap-2 pt-2">
                <button
                  type="button"
                  onClick={() => setShowSaveColModal(false)}
                  className="px-3 py-1.5 rounded-lg border border-slate-300 dark:border-slate-700 text-slate-600 dark:text-slate-300 hover:bg-slate-100 dark:hover:bg-slate-700 cursor-pointer"
                >
                  {t('export.cancel')}
                </button>
                <button
                  type="submit"
                  className="px-3.5 py-1.5 rounded-lg bg-blue-600 text-white hover:bg-blue-700 transition font-medium cursor-pointer"
                >
                  {ui('保存集合', 'Save')}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      <FilterAndColumnModal
        isOpen={showFilterModal}
        onClose={() => setShowFilterModal(false)}
        availableProfiles={Array.from(new Set([...availableProfiles, ...items.map((item) => item.profile)]))}
        availableBrowsers={Array.from(new Set([...availableBrowserIds, ...items.map((item) => item.browser)]))}
      />
      <ExportModal isOpen={showExportModal} onClose={() => setShowExportModal(false)} />
    </header>
  );
};
