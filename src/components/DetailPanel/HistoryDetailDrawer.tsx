import React, { useState, useEffect } from 'react';
import {
  X,
  ExternalLink,
  Copy,
  Check,
  Clock,
  Globe,
  Compass,
  Star,
  Tag,
  Plus,
  TrendingUp,
  History,
  Calendar,
  ShieldCheck,
  ShieldAlert,
  Archive,
  Sparkles,
  Loader2,
  Activity,
} from 'lucide-react';
import { useHistoryStore } from '../../stores/useHistoryStore';
import { useAppStore } from '../../stores/useAppStore';
import { tauriApi } from '../../services/tauri';
import { LinkHealthStatus } from '../../types';

export const HistoryDetailDrawer: React.FC = () => {
  const {
    detailVisitId,
    detailData,
    isLoadingDetail,
    closeDetail,
    setSearch,
    openDetail,
    toggleFavoriteUrl,
    addTagToUrl,
    removeTagFromUrl,
  } = useHistoryStore();
  const { showToast, t, setCurrentTab } = useAppStore();
  const [copiedKey, setCopiedKey] = useState<string | null>(null);
  const [newTagInput, setNewTagInput] = useState('');
  const [isAddingTag, setIsAddingTag] = useState(false);

  const [linkHealth, setLinkHealth] = useState<LinkHealthStatus | null>(null);
  const [isCheckingHealth, setIsCheckingHealth] = useState(false);

  useEffect(() => {
    if (!detailData?.url_id) {
      setLinkHealth(null);
      return;
    }
    let cancelled = false;
    tauriApi.getLinkHealth(detailData.url_id)
      .then((res) => {
        if (!cancelled) setLinkHealth(res);
      })
      .catch((e) => console.error('Failed to load link health:', e));
    return () => {
      cancelled = true;
    };
  }, [detailData?.url_id]);

  const handleCheckHealth = async () => {
    if (!detailData?.url_id) return;
    setIsCheckingHealth(true);
    try {
      const res = await tauriApi.checkLinkHealth(detailData.url_id);
      setLinkHealth(res);
      if (res.is_alive) {
        showToast(`链接检测存活 (HTTP ${res.status_code || 200})`, 'success');
      } else {
        showToast(`链接可能已失效 (${res.error_message || `HTTP ${res.status_code}`})`, 'warning');
      }
    } catch (err: any) {
      showToast(err?.message || '检测链接存活失败', 'error');
    } finally {
      setIsCheckingHealth(false);
    }
  };

  const handleOpenWayback = () => {
    if (!detailData?.url) return;
    const wayback = `https://web.archive.org/web/*/${encodeURI(detailData.url)}`;
    tauriApi.openExternalUrl(wayback);
  };

  if (!detailVisitId) return null;

  const handleCopy = (text: string, key: string) => {
    navigator.clipboard.writeText(text);
    setCopiedKey(key);
    showToast(t('detail.copied'), 'success');
    setTimeout(() => setCopiedKey(null), 1500);
  };

  const formatDateTime = (ms: number) => {
    return new Date(ms).toLocaleString();
  };

  const formatTimeOnly = (ms: number) => {
    const d = new Date(ms);
    const h = String(d.getHours()).padStart(2, '0');
    const m = String(d.getMinutes()).padStart(2, '0');
    const s = String(d.getSeconds()).padStart(2, '0');
    return `${h}:${m}:${s}`;
  };

  const handleFilterDomain = (domain: string) => {
    if (domain) {
      setSearch(`site:${domain}`);
      closeDetail();
    }
  };

  const handleAddTagSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!detailData || !newTagInput.trim()) return;
    try {
      await addTagToUrl(detailData.url_id, newTagInput.trim());
      setNewTagInput('');
      setIsAddingTag(false);
    } catch (err: any) {
      showToast(err?.message || '添加标签失败', 'error');
    }
  };

  const handleToggleFavorite = async () => {
    if (!detailData) return;
    try {
      const isFav = await toggleFavoriteUrl(detailData.url_id);
      showToast(isFav ? '已加入收藏' : '已取消收藏', 'info');
    } catch (err: any) {
      showToast(err?.message || '收藏操作失败', 'error');
    }
  };

  return (
    <aside className="w-96 bg-white dark:bg-slate-900 border-l border-slate-200 dark:border-slate-800 flex flex-col h-full shadow-xl z-20 shrink-0 text-xs animate-in slide-in-from-right duration-150">
      {/* Header */}
      <div className="h-11 px-3 border-b border-slate-200 dark:border-slate-800 flex items-center justify-between shrink-0">
        <span className="font-semibold text-slate-800 dark:text-slate-100 flex items-center gap-1.5">
          <History className="w-4 h-4 text-blue-500" />
          {t('detail.title')}
        </span>
        <button
          onClick={closeDetail}
          className="p-1 hover:bg-slate-100 dark:hover:bg-slate-800 rounded text-slate-500 transition"
        >
          <X className="w-4 h-4" />
        </button>
      </div>

      {isLoadingDetail || !detailData ? (
        <div className="flex-1 flex items-center justify-center text-slate-400">
          加载详情...
        </div>
      ) : (
        <div className="flex-1 overflow-y-auto p-3.5 flex flex-col gap-4">
          {/* Quick Actions & Favorite Button */}
          <div className="flex items-center gap-2">
            <button
              onClick={() => tauriApi.openExternalUrl(detailData.url)}
              className="flex-1 flex items-center justify-center gap-1.5 py-1.5 px-2 bg-blue-50 text-blue-600 dark:bg-blue-950/60 dark:text-blue-400 rounded hover:bg-blue-100 dark:hover:bg-blue-900 transition font-medium"
            >
              <ExternalLink className="w-3.5 h-3.5" />
              <span>{t('detail.openInBrowser')}</span>
            </button>

            {/* Favorite Star Button */}
            <button
              onClick={handleToggleFavorite}
              className={`flex items-center gap-1 px-2.5 py-1.5 rounded border transition font-medium ${
                detailData.is_favorite
                  ? 'bg-amber-50 dark:bg-amber-950/60 border-amber-300 dark:border-amber-700 text-amber-700 dark:text-amber-300'
                  : 'border-slate-200 dark:border-slate-700 hover:bg-slate-100 dark:hover:bg-slate-800 text-slate-700 dark:text-slate-300'
              }`}
              title={detailData.is_favorite ? '取消收藏' : '加入收藏'}
            >
              <Star
                className={`w-3.5 h-3.5 ${
                  detailData.is_favorite ? 'fill-amber-400 text-amber-500' : 'text-slate-400'
                }`}
              />
              <span>{detailData.is_favorite ? '已收藏' : '收藏'}</span>
            </button>

            {/* Copy URL */}
            <button
              onClick={() => handleCopy(detailData.url, 'url')}
              className="p-1.5 border border-slate-200 dark:border-slate-700 rounded hover:bg-slate-100 dark:hover:bg-slate-800 text-slate-700 dark:text-slate-300 transition"
              title={t('detail.copyUrl')}
            >
              {copiedKey === 'url' ? (
                <Check className="w-4 h-4 text-emerald-500" />
              ) : (
                <Copy className="w-4 h-4" />
              )}
            </button>
          </div>

          {/* Title & URL Section */}
          <div className="flex flex-col gap-1.5 p-2.5 rounded bg-slate-50 dark:bg-slate-800/40 border border-slate-200 dark:border-slate-700/60">
            <div className="flex items-start justify-between gap-2">
              <span className="font-semibold text-slate-800 dark:text-slate-100 leading-snug break-words">
                {detailData.title || '(无标题)'}
              </span>
              <button
                onClick={() => handleCopy(detailData.title, 'title')}
                className="opacity-60 hover:opacity-100 p-0.5"
                title={t('detail.copyTitle')}
              >
                <Copy className="w-3 h-3" />
              </button>
            </div>
            <span className="text-[11px] font-mono text-slate-500 dark:text-slate-400 break-all select-text">
              {detailData.url}
            </span>
          </div>

          {/* Tags Management Section */}
          <div className="flex flex-col gap-2 p-2.5 rounded bg-slate-50 dark:bg-slate-800/40 border border-slate-200 dark:border-slate-700/60">
            <div className="flex items-center justify-between">
              <span className="font-medium text-slate-700 dark:text-slate-200 flex items-center gap-1.5">
                <Tag className="w-3.5 h-3.5 text-blue-500" />
                <span>自定义标签</span>
              </span>
              {!isAddingTag && (
                <button
                  onClick={() => setIsAddingTag(true)}
                  className="flex items-center gap-1 text-[11px] text-blue-600 dark:text-blue-400 hover:underline"
                >
                  <Plus className="w-3 h-3" />
                  <span>添加标签</span>
                </button>
              )}
            </div>

            {/* Tag Pills */}
            <div className="flex flex-wrap gap-1.5 items-center">
              {detailData.tags && detailData.tags.length > 0 ? (
                detailData.tags.map((tg) => (
                  <span
                    key={tg}
                    className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full bg-blue-50 dark:bg-blue-950/60 text-blue-700 dark:text-blue-300 border border-blue-200 dark:border-blue-800 text-[11px]"
                  >
                    <span>#{tg}</span>
                    <button
                      onClick={() => removeTagFromUrl(detailData.url_id, tg)}
                      className="text-blue-400 hover:text-blue-700 dark:hover:text-blue-200"
                    >
                      <X className="w-3 h-3" />
                    </button>
                  </span>
                ))
              ) : (
                <span className="text-[11px] text-slate-400">暂无标签，方便聚类与回顾</span>
              )}
            </div>

            {/* Inline Add Tag Form */}
            {isAddingTag && (
              <form onSubmit={handleAddTagSubmit} className="flex items-center gap-1.5 pt-1">
                <input
                  type="text"
                  value={newTagInput}
                  onChange={(e) => setNewTagInput(e.target.value)}
                  placeholder="标签名 (回车确认)"
                  autoFocus
                  className="flex-1 px-2 py-1 rounded border border-slate-300 dark:border-slate-600 bg-white dark:bg-slate-900 text-xs focus:outline-none focus:border-blue-500"
                />
                <button
                  type="submit"
                  className="px-2 py-1 rounded bg-blue-600 text-white font-medium hover:bg-blue-700 transition"
                >
                  保存
                </button>
                <button
                  type="button"
                  onClick={() => {
                    setIsAddingTag(false);
                    setNewTagInput('');
                  }}
                  className="px-2 py-1 rounded border border-slate-300 dark:border-slate-600 text-slate-600 dark:text-slate-300"
                >
                  取消
                </button>
              </form>
            )}
          </div>

          {/* Visit Frequency & Lifecycle Stats */}
          <div className="grid grid-cols-2 gap-2">
            <div className="p-2.5 rounded bg-slate-50 dark:bg-slate-800/40 border border-slate-200 dark:border-slate-700/60 flex flex-col gap-1">
              <span className="text-slate-500 flex items-center gap-1 text-[11px]">
                <TrendingUp className="w-3.5 h-3.5 text-emerald-500" /> 该网址累计访问
              </span>
              <span className="text-base font-bold text-slate-800 dark:text-slate-100 font-mono">
                {detailData.total_url_visits || 1} <span className="text-xs font-normal text-slate-400">次</span>
              </span>
            </div>
            <div className="p-2.5 rounded bg-slate-50 dark:bg-slate-800/40 border border-slate-200 dark:border-slate-700/60 flex flex-col gap-1">
              <span className="text-slate-500 flex items-center gap-1 text-[11px]">
                <Calendar className="w-3.5 h-3.5 text-blue-500" /> 首次发现时间
              </span>
              <span className="text-[11px] text-slate-700 dark:text-slate-300 font-mono truncate" title={detailData.first_visit_time ? formatDateTime(detailData.first_visit_time) : '-'}>
                {detailData.first_visit_time ? new Date(detailData.first_visit_time).toLocaleDateString() : '-'}
              </span>
            </div>
          </div>

          {/* Universal Archive & Link Health (留存与链接健康) */}
          <div className="flex flex-col gap-2 p-2.5 rounded bg-slate-50 dark:bg-slate-800/40 border border-slate-200 dark:border-slate-700/60">
            <div className="flex items-center justify-between">
              <span className="font-semibold text-slate-700 dark:text-slate-200 flex items-center gap-1.5">
                <ShieldCheck className="w-3.5 h-3.5 text-blue-500" />
                <span>留存健康与历史快照</span>
              </span>
              <button
                onClick={handleCheckHealth}
                disabled={isCheckingHealth}
                className="flex items-center gap-1 text-[11px] text-blue-600 dark:text-blue-400 hover:underline cursor-pointer disabled:opacity-50"
              >
                {isCheckingHealth ? (
                  <Loader2 className="w-3 h-3 animate-spin" />
                ) : (
                  <Activity className="w-3 h-3" />
                )}
                <span>{isCheckingHealth ? '检测中...' : '即时测活'}</span>
              </button>
            </div>

            {/* Health Status Row */}
            <div className="flex items-center justify-between py-1.5 px-2 rounded bg-white dark:bg-slate-900 border border-slate-200/80 dark:border-slate-800">
              <span className="text-[11px] text-slate-500">存活可用性</span>
              {linkHealth ? (
                linkHealth.is_alive ? (
                  <span className="inline-flex items-center gap-1 text-[11px] font-medium text-emerald-600 dark:text-emerald-400">
                    <ShieldCheck className="w-3.5 h-3.5" />
                    <span>正常在线 {linkHealth.status_code ? `(${linkHealth.status_code})` : ''}</span>
                  </span>
                ) : (
                  <span className="inline-flex items-center gap-1 text-[11px] font-medium text-rose-600 dark:text-rose-400">
                    <ShieldAlert className="w-3.5 h-3.5" />
                    <span>可能失效 {linkHealth.status_code ? `(${linkHealth.status_code})` : ''}</span>
                  </span>
                )
              ) : (
                <span className="text-[11px] text-slate-400">尚未检测</span>
              )}
            </div>

            {/* Archive Actions */}
            <div className="flex items-center gap-2 pt-1">
              <button
                onClick={handleOpenWayback}
                className="flex-1 flex items-center justify-center gap-1.5 py-1.5 px-2 rounded border border-slate-300 dark:border-slate-700 bg-white dark:bg-slate-900 hover:bg-slate-50 dark:hover:bg-slate-800 text-slate-700 dark:text-slate-200 transition font-medium cursor-pointer"
                title="在互联网档案馆 (Archive.org) 查阅历史快照"
              >
                <Archive className="w-3.5 h-3.5 text-amber-500" />
                <span>Wayback 时光机快照</span>
              </button>

              <button
                onClick={() => {
                  setSearch(detailData.title || detailData.domain);
                  setCurrentTab('ai');
                  closeDetail();
                }}
                className="flex items-center justify-center gap-1 py-1.5 px-2.5 rounded border border-purple-200 dark:border-purple-800 bg-purple-50/60 dark:bg-purple-950/40 hover:bg-purple-100 text-purple-700 dark:text-purple-300 transition font-medium cursor-pointer"
                title="进入 AI 语义检索与相似网页"
              >
                <Sparkles className="w-3.5 h-3.5" />
                <span>语义检索</span>
              </button>
            </div>
          </div>

          {/* Context Nearby Visits Stream (研究复原与上下文还原) */}
          <div className="flex flex-col gap-2 p-2.5 rounded bg-slate-50 dark:bg-slate-800/40 border border-slate-200 dark:border-slate-700/60">
            <div className="flex items-center justify-between">
              <span className="font-semibold text-slate-700 dark:text-slate-200 flex items-center gap-1.5">
                <Compass className="w-3.5 h-3.5 text-indigo-500" />
                <span>附近浏览流 (前后时序还原)</span>
              </span>
              <span className="text-[10px] text-slate-400">
                {detailData.nearby_visits ? detailData.nearby_visits.length : 0} 条事件
              </span>
            </div>

            <div className="flex flex-col gap-1 mt-1 max-h-56 overflow-y-auto pr-1 divide-y divide-slate-100 dark:divide-slate-800">
              {detailData.nearby_visits && detailData.nearby_visits.length > 0 ? (
                detailData.nearby_visits.map((item) => {
                  const isCurrent = item.id === detailData.id;
                  return (
                    <div
                      key={item.id}
                      onClick={() => !isCurrent && openDetail(item.id)}
                      className={`py-1.5 px-2 rounded transition cursor-pointer flex flex-col gap-0.5 ${
                        isCurrent
                          ? 'bg-blue-100/70 dark:bg-blue-950/80 border border-blue-300 dark:border-blue-800 font-medium'
                          : 'hover:bg-slate-100 dark:hover:bg-slate-700/50'
                      }`}
                    >
                      <div className="flex items-center justify-between gap-1 text-[10px]">
                        <span className="font-mono text-slate-500 dark:text-slate-400">
                          {formatTimeOnly(item.visit_time)}
                        </span>
                        <div className="flex items-center gap-1">
                          {isCurrent && (
                            <span className="px-1.5 py-0.2 rounded bg-blue-600 text-white text-[9px]">
                              当前记录
                            </span>
                          )}
                          <span className="text-slate-400">{item.browser}</span>
                        </div>
                      </div>
                      <span className="truncate text-[11px] text-slate-800 dark:text-slate-200">
                        {item.title || item.url}
                      </span>
                    </div>
                  );
                })
              ) : (
                <div className="py-2 text-center text-slate-400 text-[11px]">
                  暂无上下文前后浏览
                </div>
              )}
            </div>
          </div>

          {/* Metadata Table */}
          <div className="flex flex-col gap-2 pt-1 border-t border-slate-200 dark:border-slate-800">
            <div className="flex items-center justify-between py-1 border-b border-slate-100 dark:border-slate-800">
              <span className="text-slate-500 flex items-center gap-1.5">
                <Clock className="w-3.5 h-3.5" /> {t('detail.visitTime')}
              </span>
              <span className="font-mono text-slate-700 dark:text-slate-200">
                {formatDateTime(detailData.visit_time)}
              </span>
            </div>

            <div className="flex items-center justify-between py-1 border-b border-slate-100 dark:border-slate-800">
              <span className="text-slate-500 flex items-center gap-1.5">
                <Globe className="w-3.5 h-3.5" /> 域名
              </span>
              <button
                onClick={() => handleFilterDomain(detailData.domain)}
                className="text-blue-600 dark:text-blue-400 hover:underline font-mono"
              >
                {detailData.domain || '-'}
              </button>
            </div>

            <div className="flex items-center justify-between py-1 border-b border-slate-100 dark:border-slate-800">
              <span className="text-slate-500 flex items-center gap-1.5">
                <Compass className="w-3.5 h-3.5" /> {t('detail.browserInfo')}
              </span>
              <span className="text-slate-700 dark:text-slate-200 font-medium">
                {detailData.browser} / {detailData.profile}
              </span>
            </div>

            <div className="flex items-center justify-between py-1 border-b border-slate-100 dark:border-slate-800">
              <span className="text-slate-500">{t('detail.visitId')}</span>
              <span className="font-mono text-slate-700 dark:text-slate-300">
                {detailData.source_visit_id}
              </span>
            </div>

            <div className="flex items-center justify-between py-1 border-b border-slate-100 dark:border-slate-800">
              <span className="text-slate-500">停留时长</span>
              <span className="font-mono text-slate-700 dark:text-slate-300">
                {detailData.visit_duration > 0 ? `${detailData.visit_duration} 秒` : '未记录'}
              </span>
            </div>

            <div className="flex items-center justify-between py-1 border-b border-slate-100 dark:border-slate-800">
              <span className="text-slate-500">{t('detail.importedAt')}</span>
              <span className="font-mono text-slate-700 dark:text-slate-300">
                {formatDateTime(detailData.imported_at)}
              </span>
            </div>

            <div className="flex flex-col gap-1 pt-1">
              <span className="text-slate-500">{t('detail.eventHash')}</span>
              <span className="font-mono text-[10px] text-slate-400 break-all select-text p-1.5 rounded bg-slate-100 dark:bg-slate-800">
                {detailData.event_hash}
              </span>
            </div>
          </div>
        </div>
      )}
    </aside>
  );
};

