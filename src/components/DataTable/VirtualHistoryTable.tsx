import React, { useRef, useEffect } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { Star } from 'lucide-react';
import { useHistoryStore } from '../../stores/useHistoryStore';
import { useAppStore } from '../../stores/useAppStore';
import { formatDurationCompact } from '../../utils/formatters';

export const VirtualHistoryTable: React.FC = () => {
  const {
    items,
    columns,
    hasMore,
    isLoading,
    historyError,
    fetchHistory,
    selectedIds,
    toggleSelectId,
    selectAllVisible,
    clearSelection,
    openDetail,
    detailVisitId,
    toggleFavoriteUrl,
  } = useHistoryStore();

  const { t, language } = useAppStore();
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
    if (items.length > 0 && lastVirtualIndex >= items.length - 15 && hasMore && !isLoading) {
      fetchHistory(false);
    }
  }, [lastVirtualIndex, items.length, hasMore, isLoading]);

  const allSelected = items.length > 0 && selectedIds.size >= items.length && items.every((i) => selectedIds.has(i.id));

  // Keep header and virtualized rows on one shared grid track definition. The
  // outer viewport owns horizontal scrolling; the body only owns vertical
  // scrolling, so long URL/title columns cannot create a second scrollbar.
  const gridTemplateColumns = [
    '32px',
    '24px',
    columns.time ? '144px' : null,
    columns.title ? 'minmax(200px, 1fr)' : null,
    columns.url ? 'minmax(240px, 1fr)' : null,
    columns.domain ? '144px' : null,
    columns.browser ? '96px' : null,
    columns.profile ? '96px' : null,
    columns.duration ? '80px' : null,
  ].filter(Boolean).join(' ');

  const formatTime = (ms: number) => {
    if (ms <= 0) return t('history.unknownTime');
    const d = new Date(ms);
    const year = d.getFullYear();
    const month = String(d.getMonth() + 1).padStart(2, '0');
    const day = String(d.getDate()).padStart(2, '0');
    const hours = String(d.getHours()).padStart(2, '0');
    const minutes = String(d.getMinutes()).padStart(2, '0');
    return `${year}-${month}-${day} ${hours}:${minutes}`;
  };

  if (items.length === 0 && !isLoading && !historyError) {
    return (
      <div className="flex-1 flex flex-col items-center justify-center text-slate-400 dark:text-slate-500 text-xs">
        <p>{t('history.empty')}</p>
      </div>
    );
  }

  return (
    <div className="flex-1 flex flex-col h-full overflow-hidden text-xs select-none">
      {historyError && (
        <div className="shrink-0 flex items-center justify-between gap-3 px-3 py-2 border-b border-rose-200 bg-rose-50 text-rose-700 dark:border-rose-900/60 dark:bg-rose-950/30 dark:text-rose-300">
          <span>{historyError}</span>
          <button
            type="button"
            onClick={() => fetchHistory(true)}
            className="shrink-0 rounded border border-rose-300 px-2 py-1 font-medium hover:bg-rose-100 dark:border-rose-800 dark:hover:bg-rose-900/50"
          >
            {t('history.retry')}
          </button>
        </div>
      )}

      {/* Header and body share one horizontal scrolling viewport. */}
      <div className="flex-1 min-w-0 overflow-x-auto overflow-y-hidden">
        <div role="grid" aria-label={t('nav.history')} className="w-max min-w-full h-full flex flex-col">
          {/* Table Header */}
          <div
            role="row"
            className="h-8 border-b border-slate-200 dark:border-slate-800 bg-slate-100 dark:bg-slate-800/60 grid items-center text-slate-500 font-medium shrink-0 px-2"
            style={{ gridTemplateColumns }}
          >
            <div role="columnheader" className="flex items-center justify-center">
              <input
                type="checkbox"
                checked={allSelected}
                aria-label={allSelected ? t('history.clearAllSelection') : t('history.selectAll')}
                onChange={() => (allSelected ? clearSelection() : selectAllVisible())}
                className="rounded border-slate-300 text-blue-600 focus:ring-0"
              />
            </div>
            <div role="columnheader" aria-label={t('history.favorite')} className="flex items-center justify-center" title={t('history.favorite')}>
              <Star className="w-3 h-3 text-slate-400" aria-hidden="true" />
            </div>

            {columns.time && <div role="columnheader" className="px-2 truncate">{t('history.columns.time')}</div>}
            {columns.title && <div role="columnheader" className="px-2 truncate">{t('history.columns.title')}</div>}
            {columns.url && <div role="columnheader" className="px-2 truncate font-mono text-[11px]">{t('history.columns.url')}</div>}

            {columns.domain && <div role="columnheader" className="px-2 truncate">{t('history.columns.domain')}</div>}
            {columns.browser && <div role="columnheader" className="px-2 truncate">{t('history.columns.browser')}</div>}
            {columns.profile && <div role="columnheader" className="px-2 truncate">{t('history.columns.profile')}</div>}
            {columns.duration && <div role="columnheader" className="px-2">{t('history.columns.duration')}</div>}
          </div>

          {/* Table Body (Virtualized) */}
          <div ref={parentRef} role="rowgroup" className="flex-1 overflow-y-auto relative divide-y divide-slate-100 dark:divide-slate-800/40 history-scrollbar">
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
                    role="row"
                    tabIndex={0}
                    aria-selected={isSelected}
                    aria-label={t('history.open', { item: item.title || item.url || t('nav.history') })}
                    onClick={() => openDetail(item.id)}
                    onKeyDown={(e) => {
                      // Inputs and buttons inside a row retain their native
                      // Space/Enter behavior; only the row itself opens detail.
                      if (e.target !== e.currentTarget) return;
                      if (e.key === 'Enter' || e.key === ' ') {
                        e.preventDefault();
                        openDetail(item.id);
                      }
                    }}
                    style={{
                      position: 'absolute',
                      top: 0,
                      left: 0,
                      width: '100%',
                      height: `${virtualRow.size}px`,
                      transform: `translateY(${virtualRow.start}px)`,
                      gridTemplateColumns,
                    }}
                    className={`grid items-center px-2 cursor-pointer border-b border-slate-100/60 dark:border-slate-800/30 transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-blue-500 ${
                      isDetailActive
                        ? 'bg-blue-50/80 dark:bg-blue-950/40 text-blue-900 dark:text-blue-100'
                        : isSelected
                        ? 'bg-slate-100 dark:bg-slate-800/70'
                        : 'hover:bg-slate-50 dark:hover:bg-slate-800/40'
                    }`}
                  >
                {/* Checkbox */}
                <div
                  role="gridcell"
                  className="w-8 flex items-center justify-center shrink-0"
                  onClick={(e) => e.stopPropagation()}
                >
                  <input
                    type="checkbox"
                    checked={isSelected}
                    aria-label={t('history.select', { item: item.title || item.url || t('nav.history') })}
                    onChange={(e) => {
                      e.stopPropagation();
                      toggleSelectId(item.id);
                    }}
                    className="rounded border-slate-300 text-blue-600 focus:ring-0"
                  />
                </div>

                {/* Favorite Star */}
                <div
                  role="gridcell"
                  className="w-6 flex items-center justify-center shrink-0"
                  onClick={(e) => {
                    e.stopPropagation();
                    toggleFavoriteUrl(item.url_id);
                  }}
                >
                  <button
                    type="button"
                    title={item.is_favorite ? t('history.unfavorite') : t('history.favorite')}
                    aria-label={item.is_favorite ? t('history.unfavorite') : t('history.favorite')}
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
                    role="gridcell"
                    className="w-36 px-2 shrink-0 font-mono text-[11px] text-slate-500 dark:text-slate-400 truncate"
                    title={new Date(item.visit_time).toLocaleString()}
                  >
                    {formatTime(item.visit_time)}
                  </div>
                )}

                {columns.title && (
                  <div role="gridcell" className="flex-1 min-w-[200px] px-2 truncate font-normal text-slate-800 dark:text-slate-200">
                    {item.title || (language === 'en-US' ? '(Untitled)' : '(无标题)')}
                  </div>
                )}

                {columns.url && (
                  <div role="gridcell" className="flex-1 min-w-[240px] px-2 truncate font-mono text-[11px] text-slate-400 dark:text-slate-500">
                    {item.url}
                  </div>
                )}

                {columns.domain && (
                  <div role="gridcell" className="w-36 px-2 truncate shrink-0 text-slate-500 font-mono text-[11px]">
                    {item.domain}
                  </div>
                )}

                {columns.browser && (
                  <div role="gridcell" className="w-24 px-2 truncate shrink-0 text-slate-600 dark:text-slate-300">
                    {item.browser}
                  </div>
                )}

                {columns.profile && (
                  <div role="gridcell" className="w-24 px-2 truncate shrink-0 text-slate-500">
                    {item.profile}
                  </div>
                )}

                {columns.duration && (
                  <div role="gridcell" className="w-20 px-2 shrink-0 text-slate-400 font-mono text-[11px]">
                    {formatDurationCompact(item.visit_duration, language)}
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
      </div>
    </div>
  );
};
