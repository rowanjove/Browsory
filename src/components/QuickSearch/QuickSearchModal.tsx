import React, { useState, useEffect, useRef } from 'react';
import {
  Search,
  Clock,
  Globe,
  CornerDownLeft,
  X,
  Sparkles,
} from 'lucide-react';
import { tauriApi } from '../../services/tauri';
import { VisitListItem } from '../../types';
import { useAppStore } from '../../stores/useAppStore';
import { useDialogFocus } from '../../hooks/useDialogFocus';
import { createRequestSequence } from '../../services/requestSequence';
import { formatRelativeTimestamp } from '../../utils/formatters';

interface QuickSearchModalProps {
  isOpen: boolean;
  onClose: () => void;
  onOpenInApp?: (item: VisitListItem) => void;
}

export const QuickSearchModal: React.FC<QuickSearchModalProps> = ({
  isOpen,
  onClose,
  onOpenInApp,
}) => {
  const { setCurrentTab, t, language } = useAppStore();
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<VisitListItem[]>([]);
  const [resultsQuery, setResultsQuery] = useState('');
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [searching, setSearching] = useState(false);
  const [searchError, setSearchError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const searchSequence = useRef(createRequestSequence());
  const dialogRef = useDialogFocus<HTMLDivElement>({
    isOpen,
    onClose,
    initialFocusRef: inputRef,
  });

  // Auto focus input on open
  useEffect(() => {
    if (isOpen) {
      setQuery('');
      setResults([]);
      setResultsQuery('');
      setSelectedIndex(0);
      setSearchError(null);
    }
  }, [isOpen]);

  // Debounced search
  useEffect(() => {
    const requestId = searchSequence.current.next();
    if (!isOpen) return;
    if (!query.trim()) {
      setResults([]);
      setResultsQuery('');
      setSearching(false);
      setSearchError(null);
      return;
    }

    // Results belong to the previous query until the new request resolves.
    // Clear them immediately so Enter cannot execute stale content.
    setResults([]);
    setResultsQuery('');
    setSearching(true);
    setSearchError(null);
    const timer = setTimeout(async () => {
      try {
        const res = await tauriApi.getHistoryPage({
          search: query.trim(),
          limit: 8,
        });
        if (!searchSequence.current.isCurrent(requestId)) return;
        setResults(res.items);
        setResultsQuery(query.trim());
        setSelectedIndex(0);
      } catch (err) {
        if (!searchSequence.current.isCurrent(requestId)) return;
        console.error('Quick search error:', err);
        setResults([]);
        setSearchError(t('quickSearch.error'));
      } finally {
        if (searchSequence.current.isCurrent(requestId)) setSearching(false);
      }
    }, 220);

    return () => clearTimeout(timer);
  }, [query, isOpen]);

  const resultRefs = useRef<(HTMLDivElement | null)[]>([]);

  useEffect(() => {
    if (results.length > 0 && resultRefs.current[selectedIndex]) {
      resultRefs.current[selectedIndex]?.scrollIntoView({ block: 'nearest' });
    }
  }, [selectedIndex, results]);

  // Keyboard navigation
  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setSelectedIndex((prev) => (results.length > 0 ? (prev + 1) % results.length : 0));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setSelectedIndex((prev) =>
        results.length > 0 ? (prev - 1 + results.length) % results.length : 0
      );
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (!searching && !searchError && resultsQuery === query.trim() && results[selectedIndex]) {
        const item = results[selectedIndex];
        if (e.ctrlKey || e.metaKey) {
          // Open in App
          if (onOpenInApp) {
            onOpenInApp(item);
          } else {
            setCurrentTab('history');
          }
          onClose();
        } else {
          // Open in Browser
          tauriApi.openExternalUrl(item.url);
          onClose();
        }
      }
    }
  };

  const formatRelativeTime = (timestamp: number) =>
    formatRelativeTimestamp(timestamp, language, t('history.unknownTime'));

  if (!isOpen) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center pt-[14vh] bg-black/40 backdrop-blur-sm animate-in fade-in duration-150"
      onClick={onClose}
      role="presentation"
    >
      <div
        ref={dialogRef}
        className="w-full max-w-2xl bg-white dark:bg-slate-800 rounded-lg shadow-2xl border border-slate-200 dark:border-slate-700 overflow-hidden"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={handleKeyDown}
        role="dialog"
        aria-modal="true"
        aria-labelledby="quick-search-title"
        tabIndex={-1}
      >
        {/* Search Input Bar */}
        <div className="flex items-center px-4 py-3.5 border-b border-slate-100 dark:border-slate-700/80 gap-3">
          <span id="quick-search-title" className="sr-only">{t('quickSearch.title')}</span>
          <Search className="w-5 h-5 text-blue-600 dark:text-blue-400 shrink-0" />
          <input
            ref={inputRef}
            role="combobox"
            aria-expanded={results.length > 0}
            aria-controls="quick-search-listbox"
            aria-activedescendant={results[selectedIndex] ? `quick-search-option-${selectedIndex}` : undefined}
            aria-label={t('quickSearch.inputLabel')}
            type="text"
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setResults([]);
              setResultsQuery('');
              setSelectedIndex(0);
            }}
            placeholder={t('quickSearch.placeholder')}
            className="flex-1 bg-transparent border-none outline-none text-slate-800 dark:text-slate-100 text-sm placeholder-slate-400 dark:placeholder-slate-500"
          />
          {query && (
            <button
              onClick={() => setQuery('')}
              aria-label={t('quickSearch.clear')}
              className="p-1 hover:bg-slate-100 dark:hover:bg-slate-700 rounded-md text-slate-400"
            >
              <X className="w-4 h-4" />
            </button>
          )}
          <div className="flex items-center gap-1.5 text-[11px] text-slate-400 dark:text-slate-500 font-mono bg-slate-100 dark:bg-slate-900 px-2 py-0.5 rounded">
            <span>{t('quickSearch.closeHint')}</span>
          </div>
        </div>

        {/* Search Results List */}
        <div
          id="quick-search-listbox"
          className="max-h-[380px] overflow-y-auto modal-scroll divide-y divide-slate-50 dark:divide-slate-700/50 p-1.5"
          role="listbox"
          aria-label={t('quickSearch.results')}
          aria-busy={searching}
        >
          {searching ? (
            <div className="py-8 text-center text-xs text-slate-400 flex items-center justify-center gap-2">
              <Sparkles className="w-4 h-4 animate-spin text-blue-500" />
              <span>{t('quickSearch.searching')}</span>
            </div>
          ) : searchError ? (
            <div className="py-8 text-center text-xs text-rose-600 dark:text-rose-400" role="alert">
              {searchError}
            </div>
          ) : results.length > 0 ? (
            results.map((item, idx) => {
              const isSelected = idx === selectedIndex;
              return (
                <div
                  key={item.id}
                  id={`quick-search-option-${idx}`}
                  ref={(el) => (resultRefs.current[idx] = el)}
                  role="option"
                  aria-selected={isSelected}
                  onClick={() => {
                    tauriApi.openExternalUrl(item.url);
                    onClose();
                  }}
                  onMouseEnter={() => setSelectedIndex(idx)}
                  className={`group flex items-center justify-between px-3.5 py-2.5 rounded-xl cursor-pointer transition-colors ${
                    isSelected
                      ? 'bg-blue-50/80 dark:bg-blue-950/40 text-blue-900 dark:text-blue-100'
                      : 'hover:bg-slate-50 dark:hover:bg-slate-700/40 text-slate-700 dark:text-slate-200'
                  }`}
                >
                  <div className="flex items-center gap-3 min-w-0 pr-3">
                    <div
                      className={`p-2 rounded-lg shrink-0 ${
                        isSelected
                          ? 'bg-blue-100 dark:bg-blue-900 text-blue-600 dark:text-blue-300'
                          : 'bg-slate-100 dark:bg-slate-900 text-slate-400'
                      }`}
                    >
                      <Globe className="w-4 h-4" />
                    </div>
                    <div className="min-w-0">
                      <div className="text-xs font-medium truncate">
                        {item.title || item.url}
                      </div>
                      <div className="text-[11px] text-slate-400 dark:text-slate-500 truncate flex items-center gap-2 mt-0.5">
                        <span className="font-mono">{item.domain}</span>
                        <span>·</span>
                        <span className="flex items-center gap-1">
                          <Clock className="w-3 h-3" />
                          {formatRelativeTime(item.visit_time)}
                        </span>
                        <span>·</span>
                        <span className="capitalize">{item.browser}</span>
                      </div>
                    </div>
                  </div>

                  <div
                    className={`flex items-center gap-1.5 shrink-0 transition-opacity ${
                      isSelected ? 'opacity-100' : 'opacity-0 group-hover:opacity-100'
                    }`}
                  >
                    {isSelected && (
                      <div className="flex items-center gap-2 text-[11px] text-blue-600 dark:text-blue-400 font-mono">
                        <span className="flex items-center gap-0.5">
                          <CornerDownLeft className="w-3 h-3" /> {t('quickSearch.open')}
                        </span>
                      </div>
                    )}
                  </div>
                </div>
              );
            })
          ) : query.trim() ? (
            <div className="py-10 text-center text-xs text-slate-400">
              {t('quickSearch.notFound', { query })}
            </div>
          ) : (
            <div className="py-8 text-center text-xs text-slate-400">
              {t('quickSearch.empty')}
            </div>
          )}
        </div>

        {/* Footer Shortcut Hints */}
        <div className="px-4 py-2 bg-slate-50 dark:bg-slate-900/60 border-t border-slate-100 dark:border-slate-700/80 flex items-center justify-between text-[11px] text-slate-400 dark:text-slate-500">
          <div className="flex items-center gap-4">
            <span className="flex items-center gap-1">
              <kbd className="px-1.5 py-0.5 bg-white dark:bg-slate-800 rounded border border-slate-200 dark:border-slate-700 font-mono">
                ↑↓
              </kbd>{' '}
              {t('quickSearch.navigate')}
            </span>
            <span className="flex items-center gap-1">
              <kbd className="px-1.5 py-0.5 bg-white dark:bg-slate-800 rounded border border-slate-200 dark:border-slate-700 font-mono">
                Enter
              </kbd>{' '}
              {t('quickSearch.browserOpen')}
            </span>
            <span className="flex items-center gap-1">
              <kbd className="px-1.5 py-0.5 bg-white dark:bg-slate-800 rounded border border-slate-200 dark:border-slate-700 font-mono">
                Ctrl + Enter
              </kbd>{' '}
              {t('quickSearch.appOpen')}
            </span>
          </div>
          <div className="flex items-center gap-1">
            <Sparkles className="w-3 h-3 text-indigo-400" />
            <span>Browsory Spotlight</span>
          </div>
        </div>
      </div>
    </div>
  );
};
