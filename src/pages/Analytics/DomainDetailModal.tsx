import React, { useEffect, useState } from 'react';
import {
  X,
  Globe,
  ExternalLink,
  Clock,
  Link2,
  ChevronRight,
  ChevronDown,
  Folder,
  FileText,
  Search,
  Loader2,
} from 'lucide-react';
import { tauriApi } from '../../services/tauri';
import { DomainDetail, PathTreeNode } from '../../types';
import { useHistoryStore } from '../../stores/useHistoryStore';
import { useAppStore } from '../../stores/useAppStore';

interface DomainDetailModalProps {
  domain: string | null;
  onClose: () => void;
  startTime?: number;
  endTime?: number;
}

const PathNodeItem: React.FC<{ node: PathTreeNode; level?: number }> = ({ node, level = 0 }) => {
  const [isOpen, setIsOpen] = useState(level < 1);
  const hasChildren = node.children && node.children.length > 0;

  return (
    <div className="flex flex-col text-xs font-mono select-none">
      <div
        className={`flex items-center justify-between py-1 px-1.5 rounded hover:bg-slate-100 dark:hover:bg-slate-800 transition ${
          level > 0 ? 'ml-3' : ''
        }`}
      >
        <div
          className="flex items-center gap-1.5 cursor-pointer truncate flex-1"
          onClick={() => hasChildren && setIsOpen(!isOpen)}
        >
          {hasChildren ? (
            isOpen ? (
              <ChevronDown className="w-3.5 h-3.5 text-slate-400 shrink-0" />
            ) : (
              <ChevronRight className="w-3.5 h-3.5 text-slate-400 shrink-0" />
            )
          ) : (
            <span className="w-3.5 shrink-0" />
          )}

          {hasChildren ? (
            <Folder className="w-3.5 h-3.5 text-amber-500 shrink-0" />
          ) : (
            <FileText className="w-3.5 h-3.5 text-slate-400 shrink-0" />
          )}

          <span className="text-slate-800 dark:text-slate-200 truncate">{node.name || '/'}</span>
        </div>

        <div className="flex items-center gap-3 text-[11px] text-slate-400 shrink-0">
          <span>{node.unique_urls} 个 URL</span>
          <span className="text-blue-600 dark:text-blue-400 font-semibold">{node.visits} 次</span>
        </div>
      </div>

      {hasChildren && isOpen && (
        <div className="border-l border-slate-200 dark:border-slate-700 ml-3 pl-1 flex flex-col gap-0.5">
          {node.children.map((child, idx) => (
            <PathNodeItem key={`${child.full_path}-${idx}`} node={child} level={level + 1} />
          ))}
        </div>
      )}
    </div>
  );
};

export const DomainDetailModal: React.FC<DomainDetailModalProps> = ({
  domain,
  onClose,
  startTime,
  endTime,
}) => {
  const { setSearch } = useHistoryStore();
  const { setCurrentTab } = useAppStore();

  const [detail, setDetail] = useState<DomainDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [activeTab, setActiveTab] = useState<'overview' | 'tree' | 'urls'>('overview');

  useEffect(() => {
    if (!domain) return;
    setLoading(true);
    tauriApi
      .getDomainDetail(domain, startTime, endTime)
      .then((res) => {
        setDetail(res);
      })
      .catch((e) => {
        console.error('Failed to load domain detail', e);
      })
      .finally(() => {
        setLoading(false);
      });
  }, [domain, startTime, endTime]);

  if (!domain) return null;

  const handleSearchSite = () => {
    setSearch(`site:${domain}`);
    setCurrentTab('history');
    onClose();
  };

  const formatTime = (ts: number) => {
    if (!ts) return '-';
    return new Date(ts).toLocaleString('zh-CN', {
      year: 'numeric',
      month: '2-digit',
      day: '2-digit',
      hour: '2-digit',
      minute: '2-digit',
    });
  };

  const maxHourlyCount = Math.max(...(detail?.hourly_distribution.map((h) => h.count) || [1]), 1);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-sm p-4 text-xs">
      <div className="bg-white dark:bg-slate-900 border border-slate-200 dark:border-slate-800 rounded-xl shadow-2xl w-full max-w-2xl max-h-[85vh] flex flex-col overflow-hidden">
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-slate-200 dark:border-slate-800 bg-slate-50/50 dark:bg-slate-800/50">
          <div className="flex items-center gap-2.5">
            <div className="w-8 h-8 rounded-lg bg-blue-100 dark:bg-blue-950/60 text-blue-600 dark:text-blue-400 flex items-center justify-center font-bold">
              <Globe className="w-4 h-4" />
            </div>
            <div>
              <h2 className="font-semibold text-slate-900 dark:text-slate-100 text-sm flex items-center gap-2">
                <span>{domain}</span>
              </h2>
              <p className="text-[11px] text-slate-400">域名访问指标与 URL 路径结构</p>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <button
              onClick={handleSearchSite}
              className="flex items-center gap-1.5 px-2.5 py-1 rounded bg-blue-50 text-blue-600 dark:bg-blue-950/60 dark:text-blue-400 border border-blue-200 dark:border-blue-900 hover:bg-blue-100 transition"
              title="在主检索中筛选该网站"
            >
              <Search className="w-3.5 h-3.5" />
              <span>搜索该站</span>
            </button>
            <button
              onClick={onClose}
              className="p-1 text-slate-400 hover:text-slate-600 dark:hover:text-slate-200 rounded"
            >
              <X className="w-4 h-4" />
            </button>
          </div>
        </div>

        {/* Tab navigation */}
        <div className="flex items-center gap-4 px-6 border-b border-slate-200 dark:border-slate-800 text-xs">
          <button
            onClick={() => setActiveTab('overview')}
            className={`py-2.5 border-b-2 font-medium transition ${
              activeTab === 'overview'
                ? 'border-blue-600 text-blue-600 dark:border-blue-400 dark:text-blue-400'
                : 'border-transparent text-slate-500 hover:text-slate-700 dark:text-slate-400'
            }`}
          >
            概览与时段分布
          </button>
          <button
            onClick={() => setActiveTab('tree')}
            className={`py-2.5 border-b-2 font-medium transition flex items-center gap-1.5 ${
              activeTab === 'tree'
                ? 'border-blue-600 text-blue-600 dark:border-blue-400 dark:text-blue-400'
                : 'border-transparent text-slate-500 hover:text-slate-700 dark:text-slate-400'
            }`}
          >
            <Folder className="w-3.5 h-3.5" />
            <span>URL 路径树</span>
          </button>
          <button
            onClick={() => setActiveTab('urls')}
            className={`py-2.5 border-b-2 font-medium transition flex items-center gap-1.5 ${
              activeTab === 'urls'
                ? 'border-blue-600 text-blue-600 dark:border-blue-400 dark:text-blue-400'
                : 'border-transparent text-slate-500 hover:text-slate-700 dark:text-slate-400'
            }`}
          >
            <Link2 className="w-3.5 h-3.5" />
            <span>Top 访问网页 ({detail?.top_urls.length || 0})</span>
          </button>
        </div>

        {/* Content area */}
        <div className="flex-1 overflow-y-auto p-6">
          {loading ? (
            <div className="flex flex-col items-center justify-center h-48 gap-2 text-slate-400">
              <Loader2 className="w-6 h-6 animate-spin text-blue-600" />
              <span>加载域名指标与路径树中...</span>
            </div>
          ) : detail ? (
            <>
              {activeTab === 'overview' && (
                <div className="flex flex-col gap-6">
                  {/* KPI Cards */}
                  <div className="grid grid-cols-4 gap-3">
                    <div className="p-3.5 rounded-lg border border-slate-200 dark:border-slate-800 bg-slate-50/60 dark:bg-slate-800/40">
                      <div className="text-[11px] text-slate-400 mb-1">总访问量</div>
                      <div className="text-xl font-bold text-slate-800 dark:text-slate-100 font-mono">
                        {detail.total_visits}
                      </div>
                    </div>
                    <div className="p-3.5 rounded-lg border border-slate-200 dark:border-slate-800 bg-slate-50/60 dark:bg-slate-800/40">
                      <div className="text-[11px] text-slate-400 mb-1">独立网页数</div>
                      <div className="text-xl font-bold text-slate-800 dark:text-slate-100 font-mono">
                        {detail.unique_urls}
                      </div>
                    </div>
                    <div className="p-3.5 rounded-lg border border-slate-200 dark:border-slate-800 bg-slate-50/60 dark:bg-slate-800/40">
                      <div className="text-[11px] text-slate-400 mb-1">首次访问</div>
                      <div className="text-xs font-medium text-slate-700 dark:text-slate-300">
                        {formatTime(detail.first_visit_time)}
                      </div>
                    </div>
                    <div className="p-3.5 rounded-lg border border-slate-200 dark:border-slate-800 bg-slate-50/60 dark:bg-slate-800/40">
                      <div className="text-[11px] text-slate-400 mb-1">最近一次访问</div>
                      <div className="text-xs font-medium text-slate-700 dark:text-slate-300">
                        {formatTime(detail.last_visit_time)}
                      </div>
                    </div>
                  </div>

                  {/* Hourly 24h Distribution */}
                  <div className="p-4 rounded-lg border border-slate-200 dark:border-slate-800 bg-white dark:bg-slate-850">
                    <div className="font-semibold text-slate-800 dark:text-slate-100 mb-3 flex items-center gap-2">
                      <Clock className="w-3.5 h-3.5 text-blue-500" />
                      <span>全天 24 小时访问时段分布</span>
                    </div>
                    <div className="flex items-end gap-1.5 h-28 pt-2">
                      {detail.hourly_distribution.map((h) => {
                        const heightPercent = maxHourlyCount > 0 ? (h.count / maxHourlyCount) * 100 : 0;
                        return (
                          <div key={h.hour} className="flex-1 flex flex-col items-center gap-1 group relative">
                            <div className="w-full bg-slate-100 dark:bg-slate-800 rounded-t h-full flex items-end">
                              <div
                                style={{ height: `${heightPercent}%` }}
                                className="w-full bg-blue-500 hover:bg-blue-600 rounded-t transition-all"
                              />
                            </div>
                            <span className="text-[10px] text-slate-400">{h.hour}</span>

                            {/* Tooltip */}
                            <div className="absolute -top-7 hidden group-hover:flex items-center px-1.5 py-0.5 rounded bg-slate-800 text-white text-[10px] pointer-events-none whitespace-nowrap z-10">
                              {h.hour}点: {h.count} 次
                            </div>
                          </div>
                        );
                      })}
                    </div>
                  </div>
                </div>
              )}

              {activeTab === 'tree' && (
                <div className="flex flex-col gap-3">
                  <div className="flex items-center justify-between text-slate-500 text-[11px]">
                    <span>按 URL 路径层级自动归类聚合：</span>
                    <span>共 {detail.path_tree.length} 个根路径分支</span>
                  </div>

                  {detail.path_tree.length === 0 ? (
                    <div className="text-center py-12 text-slate-400">暂无层级子路径数据</div>
                  ) : (
                    <div className="p-3 rounded-lg border border-slate-200 dark:border-slate-800 bg-slate-50/50 dark:bg-slate-850 flex flex-col gap-1 max-h-[450px] overflow-y-auto">
                      {detail.path_tree.map((node, idx) => (
                        <PathNodeItem key={`${node.full_path}-${idx}`} node={node} level={0} />
                      ))}
                    </div>
                  )}
                </div>
              )}

              {activeTab === 'urls' && (
                <div className="flex flex-col gap-2">
                  {detail.top_urls.length === 0 ? (
                    <div className="text-center py-12 text-slate-400">暂无热门网页记录</div>
                  ) : (
                    <div className="flex flex-col divide-y divide-slate-100 dark:divide-slate-800">
                      {detail.top_urls.map((u, i) => (
                        <div key={i} className="py-2.5 flex items-center justify-between gap-4 group">
                          <div className="flex flex-col gap-0.5 truncate flex-1">
                            <div className="font-medium text-slate-800 dark:text-slate-200 truncate text-xs">
                              {u.title || '(无标题)'}
                            </div>
                            <a
                              href={u.url}
                              onClick={(e) => {
                                e.preventDefault();
                                tauriApi.openExternalUrl(u.url);
                              }}
                              className="text-[11px] text-blue-500 dark:text-blue-400 hover:underline truncate flex items-center gap-1 font-mono"
                            >
                              <span className="truncate">{u.url}</span>
                              <ExternalLink className="w-2.5 h-2.5 shrink-0 opacity-0 group-hover:opacity-100 transition" />
                            </a>
                          </div>

                          <div className="flex items-center gap-4 shrink-0 text-[11px] text-slate-400">
                            <span>{formatTime(u.last_visit_time)}</span>
                            <span className="px-2 py-0.5 rounded-full bg-blue-50 dark:bg-blue-950 text-blue-600 dark:text-blue-400 font-semibold font-mono">
                              {u.visits} 次
                            </span>
                          </div>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              )}
            </>
          ) : (
            <div className="text-center py-12 text-slate-400">未能获取到该域名的分析详情</div>
          )}
        </div>
      </div>
    </div>
  );
};
