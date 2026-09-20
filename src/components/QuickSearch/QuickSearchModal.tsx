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
  const { setCurrentTab } = useAppStore();
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<VisitListItem[]>([]);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [searching, setSearching] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  // Auto focus input on open
  useEffect(() => {
    if (isOpen) {
      setQuery('');
      setResults([]);
      setSelectedIndex(0);
      setTimeout(() => inputRef.current?.focus(), 50);
    }
  }, [isOpen]);

  // Debounced search
  useEffect(() => {
    if (!isOpen) return;
    if (!query.trim()) {
      setResults([]);
      setSearching(false);
      return;
    }

    setSearching(true);
    const timer = setTimeout(async () => {
      try {
        const res = await tauriApi.getHistoryPage({
          search: query.trim(),
          limit: 8,
        });
        setResults(res.items);
        setSelectedIndex(0);
      } catch (err) {
        console.error('Quick search error:', err);
      } finally {
        setSearching(false);
      }
    }, 150);

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
    if (e.key === 'Escape') {
      e.preventDefault();
      onClose();
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      setSelectedIndex((prev) => (results.length > 0 ? (prev + 1) % results.length : 0));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setSelectedIndex((prev) =>
        results.length > 0 ? (prev - 1 + results.length) % results.length : 0
      );
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (results[selectedIndex]) {
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

  const formatRelativeTime = (timestamp: number) => {
    const diff = Date.now() - timestamp;
    const mins = Math.floor(diff / 60000);
    const hours = Math.floor(diff / 3600000);
    const days = Math.floor(diff / 86400000);

    if (mins < 1) return '刚刚';
    if (mins < 60) return `${mins} 分钟前`;
    if (hours < 24) return `${hours} 小时前`;
    if (days === 1) return '昨天';
    if (days < 30) return `${days} 天前`;
    return new Date(timestamp).toLocaleDateString();
  };

  if (!isOpen) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center pt-[14vh] bg-black/40 backdrop-blur-sm animate-in fade-in duration-150"
      onClick={onClose}
    >
      <div
        className="w-full max-w-2xl bg-white dark:bg-slate-800 rounded-2xl shadow-2xl border border-slate-200 dark:border-slate-700 overflow-hidden"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={handleKeyDown}
      >
        {/* Search Input Bar */}
        <div className="flex items-center px-4 py-3.5 border-b border-slate-100 dark:border-slate-700/80 gap-3">
          <Search className="w-5 h-5 text-indigo-500 shrink-0" />
          <input
            ref={inputRef}
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="搜索历史记录 (支持标题、网址、域名全文检索)..."
            className="flex-1 bg-transparent border-none outline-none text-slate-800 dark:text-slate-100 text-sm placeholder-slate-400 dark:placeholder-slate-500"
          />
          {query && (
            <button
              onClick={() => setQuery('')}
              className="p-1 hover:bg-slate-100 dark:hover:bg-slate-700 rounded-md text-slate-400"
            >
              <X className="w-4 h-4" />
            </button>
          )}
          <div className="flex items-center gap-1.5 text-[11px] text-slate-400 dark:text-slate-500 font-mono bg-slate-100 dark:bg-slate-900 px-2 py-0.5 rounded">
            <span>ESC 关闭</span>
          </div>
        </div>

        {/* Search Results List */}
        <div className="max-h-[380px] overflow-y-auto divide-y divide-slate-50 dark:divide-slate-700/50 p-1.5">
          {searching ? (
            <div className="py-8 text-center text-xs text-slate-400 flex items-center justify-center gap-2">
              <Sparkles className="w-4 h-4 animate-spin text-indigo-500" />
              <span>正在检索历史记录...</span>
            </div>
          ) : results.length > 0 ? (
            results.map((item, idx) => {
              const isSelected = idx === selectedIndex;
              return (
                <div
                  key={item.id}
                  ref={(el) => (resultRefs.current[idx] = el)}
                  onClick={() => {
                    tauriApi.openExternalUrl(item.url);
                    onClose();
                  }}
                  onMouseEnter={() => setSelectedIndex(idx)}
                  className={`flex items-center justify-between px-3.5 py-2.5 rounded-xl cursor-pointer transition-colors ${
                    isSelected
                      ? 'bg-indigo-50/80 dark:bg-indigo-950/40 text-indigo-900 dark:text-indigo-100'
                      : 'hover:bg-slate-50 dark:hover:bg-slate-700/40 text-slate-700 dark:text-slate-200'
                  }`}
                >
                  <div className="flex items-center gap-3 min-w-0 pr-3">
                    <div
                      className={`p-2 rounded-lg shrink-0 ${
                        isSelected
                          ? 'bg-indigo-100 dark:bg-indigo-900 text-indigo-600 dark:text-indigo-300'
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

                  <div className="flex items-center gap-1.5 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity">
                    {isSelected && (
                      <div className="flex items-center gap-2 text-[11px] text-indigo-600 dark:text-indigo-400 font-mono">
                        <span className="flex items-center gap-0.5">
                          <CornerDownLeft className="w-3 h-3" /> 打开
                        </span>
                      </div>
                    )}
                  </div>
                </div>
              );
            })
          ) : query.trim() ? (
            <div className="py-10 text-center text-xs text-slate-400">
              未找到与 "{query}" 匹配的历史记录
            </div>
          ) : (
            <div className="py-8 text-center text-xs text-slate-400">
              输入关键词即时检索历史页面，支持标题、网址与域名搜索
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
              导航
            </span>
            <span className="flex items-center gap-1">
              <kbd className="px-1.5 py-0.5 bg-white dark:bg-slate-800 rounded border border-slate-200 dark:border-slate-700 font-mono">
                Enter
              </kbd>{' '}
              浏览器打开
            </span>
            <span className="flex items-center gap-1">
              <kbd className="px-1.5 py-0.5 bg-white dark:bg-slate-800 rounded border border-slate-200 dark:border-slate-700 font-mono">
                Ctrl + Enter
              </kbd>{' '}
              Browsory 查看
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
