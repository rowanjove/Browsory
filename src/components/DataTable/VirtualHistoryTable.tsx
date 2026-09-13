import React, { useRef, useEffect } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { Star } from 'lucide-react';
import { useHistoryStore } from '../../stores/useHistoryStore';
import { useAppStore } from '../../stores/useAppStore';

export const VirtualHistoryTable: React.FC = () => {
  const {
    items,
    columns,
    hasMore,
    isLoading,
    fetchHistory,
    selectedIds,
    toggleSelectId,
    selectAllVisible,
    clearSelection,
    openDetail,
    detailVisitId,
    toggleFavoriteUrl,
  } = useHistoryStore();

  const { t } = useAppStore();
  const parentRef = useRef<HTMLDivElement>(null);

  // Initialize data on mount
  useEffect(() => {
    if (items.length === 0) {
      fetchHistory(true);
    }
  }, []);

  const rowVirtualizer = useVirtualizer({
    count: items.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 32, // Compact Windows desktop row height
    overscan: 25,
  });

  const virtualItems = rowVirtualizer.getVirtualItems();
  const lastVirtualIndex = virtualItems.length > 0 ? virtualItems[virtualItems.length - 1].index : -1;

  // Infinite scroll trigger
  useEffect(() => {
    if (lastVirtualIndex >= items.length - 15 && hasMore && !isLoading) {
      fetchHistory(false);
    }
  }, [lastVirtualIndex, items.length, hasMore, isLoading]);

  const allSelected = items.length > 0 && selectedIds.size >= items.length && items.every((i) => selectedIds.has(i.id));

  const formatTime = (ms: number) => {
    const d = new Date(ms);
    const year = d.getFullYear();
    const month = String(d.getMonth() + 1).padStart(2, '0');
    const day = String(d.getDate()).padStart(2, '0');
    const hours = String(d.getHours()).padStart(2, '0');
    const minutes = String(d.getMinutes()).padStart(2, '0');
    return `${year}-${month}-${day} ${hours}:${minutes}`;
  };

  if (items.length === 0 && !isLoading) {
    return (
      <div className="flex-1 flex flex-col items-center justify-center text-slate-400 dark:text-slate-500 text-xs">
        <p>{t('history.empty')}</p>
      </div>
    );
  }

  return (
    <div className="flex-1 flex flex-col h-full overflow-hidden text-xs select-none">
      {/* Table Header */}
      <div className="h-8 border-b border-slate-200 dark:border-slate-800 bg-slate-100 dark:bg-slate-800/60 flex items-center text-slate-500 font-medium shrink-0 px-2">
        <div className="w-8 flex items-center justify-center">
          <input
            type="checkbox"
            checked={allSelected}
            onChange={() => (allSelected ? clearSelection() : selectAllVisible())}
            className="rounded border-slate-300 text-blue-600 focus:ring-0"
          />
        </div>
        <div className="w-6 flex items-center justify-center shrink-0" title="收藏">
          <Star className="w-3 h-3 text-slate-400" />
        </div>

        {columns.time && <div className="w-36 px-2 shrink-0">{t('history.columns.time')}</div>}
        {columns.title && <div className="flex-1 min-w-[200px] px-2 truncate">{t('history.columns.title')}</div>}
        {columns.url && <div className="flex-1 min-w-[240px] px-2 truncate font-mono text-[11px]">{t('history.columns.url')}</div>}

        {columns.domain && <div className="w-36 px-2 truncate shrink-0">{t('history.columns.domain')}</div>}
        {columns.browser && <div className="w-24 px-2 truncate shrink-0">{t('history.columns.browser')}</div>}
        {columns.profile && <div className="w-24 px-2 truncate shrink-0">{t('history.columns.profile')}</div>}
        {columns.duration && <div className="w-20 px-2 shrink-0">{t('history.columns.duration')}</div>}
      </div>

      {/* Table Body (Virtualized) */}
      <div ref={parentRef} className="flex-1 overflow-y-auto relative divide-y divide-slate-100 dark:divide-slate-800/40 history-scrollbar">
        <div
          style={{
            height: `${rowVirtualizer.getTotalSize()}px`,
            width: '100%',
            position: 'relative',
          }}
        >
          {rowVirtualizer.getVirtualItems().map((virtualRow) => {
            const item = items[virtualRow.index];
            if (!item) return null;

            const isSelected = selectedIds.has(item.id);
            const isDetailActive = detailVisitId === item.id;

            return (
              <div
                key={item.id}
                onClick={() => openDetail(item.id)}
                style={{
                  position: 'absolute',
                  top: 0,
                  left: 0,
                  width: '100%',
                  height: `${virtualRow.size}px`,
                  transform: `translateY(${virtualRow.start}px)`,
                }}
                className={`flex items-center px-2 cursor-pointer border-b border-slate-100/60 dark:border-slate-800/30 transition-colors ${
                  isDetailActive
                    ? 'bg-blue-50/80 dark:bg-blue-950/40 text-blue-900 dark:text-blue-100'
                    : isSelected
                    ? 'bg-slate-100 dark:bg-slate-800/70'
                    : 'hover:bg-slate-50 dark:hover:bg-slate-800/40'
                }`}
              >
                {/* Checkbox */}
                <div
                  className="w-8 flex items-center justify-center shrink-0"
                  onClick={(e) => {
                    e.stopPropagation();
                    toggleSelectId(item.id);
                  }}
                >
                  <input
                    type="checkbox"
                    checked={isSelected}
                    onChange={() => {}}
                    className="rounded border-slate-300 text-blue-600 focus:ring-0"
                  />
                </div>

                {/* Favorite Star */}
                <div
                  className="w-6 flex items-center justify-center shrink-0"
                  onClick={(e) => {
                    e.stopPropagation();
                    toggleFavoriteUrl(item.url_id);
                  }}
                >
                  <button
                    type="button"
                    title={item.is_favorite ? '取消收藏' : '加入收藏'}
                    className="p-1 rounded hover:bg-slate-200 dark:hover:bg-slate-700 transition"
                  >
                    <Star
                      className={`w-3.5 h-3.5 transition-colors ${
                        item.is_favorite
                          ? 'fill-amber-400 text-amber-500'
                          : 'text-slate-300 dark:text-slate-600 hover:text-amber-400'
                      }`}
                    />
                  </button>
                </div>

                {/* Columns */}
                {columns.time && (
                  <div
                    className="w-36 px-2 shrink-0 font-mono text-[11px] text-slate-500 dark:text-slate-400 truncate"
                    title={new Date(item.visit_time).toLocaleString()}
                  >
                    {formatTime(item.visit_time)}
                  </div>
                )}

                {columns.title && (
                  <div className="flex-1 min-w-[200px] px-2 truncate font-normal text-slate-800 dark:text-slate-200">
                    {item.title || '(无标题)'}
                  </div>
                )}

                {columns.url && (
                  <div className="flex-1 min-w-[240px] px-2 truncate font-mono text-[11px] text-slate-400 dark:text-slate-500">
                    {item.url}
                  </div>
                )}

                {columns.domain && (
                  <div className="w-36 px-2 truncate shrink-0 text-slate-500 font-mono text-[11px]">
                    {item.domain}
                  </div>
                )}

                {columns.browser && (
                  <div className="w-24 px-2 truncate shrink-0 text-slate-600 dark:text-slate-300">
                    {item.browser}
                  </div>
                )}

                {columns.profile && (
                  <div className="w-24 px-2 truncate shrink-0 text-slate-500">
                    {item.profile}
                  </div>
                )}

                {columns.duration && (
                  <div className="w-20 px-2 shrink-0 text-slate-400 font-mono text-[11px]">
                    {item.visit_duration > 0 ? `${item.visit_duration}s` : '-'}
                  </div>
                )}
              </div>
            );
          })}
        </div>

        {/* Loading Indicator */}
        {isLoading && (
          <div className="p-2 text-center text-slate-400 text-xs">
            {t('history.loading')}
          </div>
        )}
      </div>
    </div>
  );
};
