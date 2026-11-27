import React, { useState, useEffect, useRef } from 'react';
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
  BookOpen,
} from 'lucide-react';
import { useHistoryStore } from '../../stores/useHistoryStore';
import { useAppStore } from '../../stores/useAppStore';
import { tauriApi } from '../../services/tauri';
import { LinkHealthStatus } from '../../types';
import { OfflineReaderModal } from './OfflineReaderModal';
import { createRequestSequence } from '../../services/requestSequence';
import { formatDurationCompact } from '../../utils/formatters';

export const HistoryDetailDrawer: React.FC = () => {
  const {
    detailVisitId,
    detailData,
    isLoadingDetail,
    detailError,
    closeDetail,
    setSearch,
    openDetail,
    toggleFavoriteUrl,
    addTagToUrl,
    removeTagFromUrl,
  } = useHistoryStore();
  const { showToast, t, setCurrentTab, language } = useAppStore();
  const ui = (zh: string, en: string) => (language === 'en-US' ? en : zh);
  const [copiedKey, setCopiedKey] = useState<string | null>(null);
  const [newTagInput, setNewTagInput] = useState('');
  const [isAddingTag, setIsAddingTag] = useState(false);

  const [linkHealth, setLinkHealth] = useState<LinkHealthStatus | null>(null);
  const [isCheckingHealth, setIsCheckingHealth] = useState(false);
  const [healthError, setHealthError] = useState<string | null>(null);
  const healthSequence = useRef(createRequestSequence());
  const [isOfflineReaderOpen, setIsOfflineReaderOpen] = useState(false);

  useEffect(() => {
    const request = healthSequence.current.next();
    setLinkHealth(null);
    setHealthError(null);
    setIsCheckingHealth(false);
    if (!detailData?.url_id) return;
    const urlId = detailData.url_id;
    void tauriApi.getLinkHealth(urlId)
      .then((res) => {
        if (healthSequence.current.isCurrent(request)) setLinkHealth(res);
      })
      .catch((error) => {
        if (healthSequence.current.isCurrent(request)) setHealthError(typeof error === 'string' ? error : t('detail.healthLoadFailed'));
      });
    return () => { healthSequence.current.next(); };
  }, [detailData?.url_id, detailVisitId]);

  const handleCheckHealth = async () => {
    if (!detailData?.url_id) return;
    const urlId = detailData.url_id;
    const request = healthSequence.current.next();
    const isCurrent = () => healthSequence.current.isCurrent(request)
      && useHistoryStore.getState().detailData?.url_id === urlId;
    setIsCheckingHealth(true);
    setHealthError(null);
    try {
      const res = await tauriApi.checkLinkHealth(urlId);
      if (!isCurrent()) return;
      setLinkHealth(res);
      showToast(res.is_alive
        ? t('detail.healthAlive', { status: res.status_code || 200 })
        : t('detail.healthUnavailable', { reason: res.error_message || `HTTP ${res.status_code}` }),
        res.is_alive ? 'success' : 'warning');
    } catch (error) {
      if (isCurrent()) setHealthError(typeof error === 'string' ? error : t('detail.healthCheckFailed'));
    } finally {
      if (isCurrent()) setIsCheckingHealth(false);
    }
  };

  const handleOpenWayback = () => {
    if (!detailData?.url) return;
    const wayback = `https://web.archive.org/web/*/${encodeURI(detailData.url)}`;
    tauriApi.openExternalUrl(wayback);
  };

  if (detailVisitId === null) return null;

  const handleCopy = async (text: string, key: string) => {
    try {
      await navigator.clipboard.writeText(text);
      setCopiedKey(key);
      showToast(t('detail.copied'), 'success');
      setTimeout(() => setCopiedKey(null), 1500);
    } catch {
      showToast(t('detail.copyFailed'), 'error');
    }
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
      showToast(err?.message || ui('添加标签失败', 'Failed to add tag'), 'error');
    }
  };

  const handleToggleFavorite = async () => {
    if (!detailData) return;
    try {
      const isFav = await toggleFavoriteUrl(detailData.url_id);
      showToast(isFav ? ui('已加入收藏', 'Added to favorites') : ui('已取消收藏', 'Removed from favorites'), 'info');
    } catch (err: any) {
      showToast(err?.message || ui('收藏操作失败', 'Failed to update favorite'), 'error');
    }
  };

  return (
    <aside className="w-96 max-w-[calc(100vw-1rem)] bg-white dark:bg-slate-900 border-l border-slate-200 dark:border-slate-800 flex flex-col h-full shadow-xl z-20 shrink-0 text-xs animate-in slide-in-from-right duration-150 max-[1100px]:absolute max-[1100px]:inset-y-0 max-[1100px]:right-0 max-[1100px]:h-full">
      {/* Header */}
      <div className="h-11 px-3 border-b border-slate-200 dark:border-slate-800 flex items-center justify-between shrink-0">
        <span className="font-semibold text-slate-800 dark:text-slate-100 flex items-center gap-1.5">
          <History className="w-4 h-4 text-blue-500" />
          {t('detail.title')}
        </span>
        <button
          onClick={closeDetail}
          aria-label={ui('关闭详情', 'Close detail drawer')}
          className="p-1 hover:bg-slate-100 dark:hover:bg-slate-800 rounded text-slate-500 transition cursor-pointer"
        >
          <X className="w-4 h-4" />
        </button>
      </div>

      {isLoadingDetail ? (
        <div className="flex-1 flex items-center justify-center text-slate-400">
          {t('history.loading')}
        </div>
      ) : detailError ? (
        <div className="flex-1 flex flex-col items-center justify-center gap-3 px-5 text-center text-rose-600 dark:text-rose-300">
          <p>{detailError}</p>
          <button
            type="button"
            onClick={() => detailVisitId !== null && openDetail(detailVisitId)}
            className="rounded border border-rose-300 px-3 py-1.5 font-medium hover:bg-rose-50 dark:border-rose-800 dark:hover:bg-rose-950/40 cursor-pointer"
          >
            {t('common.retry')}
          </button>
        </div>
      ) : !detailData ? (
        <div className="flex-1 flex items-center justify-center text-slate-400">
          {ui('暂无详情数据', 'No visit details available')}
        </div>
      ) : (
        <div className="flex-1 overflow-y-auto p-3.5 flex flex-col gap-4">
          {/* Quick Actions & Favorite Button */}
          <div className="flex items-center gap-2">
            <button
              onClick={() => tauriApi.openExternalUrl(detailData.url)}
              className="flex-1 flex items-center justify-center gap-1.5 py-1.5 px-2 bg-blue-50 text-blue-600 dark:bg-blue-950/60 dark:text-blue-400 rounded hover:bg-blue-100 dark:hover:bg-blue-900 transition font-medium cursor-pointer"
            >
              <ExternalLink className="w-3.5 h-3.5" />
              <span>{t('detail.openInBrowser')}</span>
            </button>

            <button
              onClick={handleToggleFavorite}
              className={`flex items-center gap-1 px-2.5 py-1.5 rounded border transition font-medium cursor-pointer ${
                detailData.is_favorite
                  ? 'bg-amber-50 dark:bg-amber-950/60 border-amber-300 dark:border-amber-700 text-amber-700 dark:text-amber-300'
                  : 'border-slate-200 dark:border-slate-700 hover:bg-slate-100 dark:hover:bg-slate-800 text-slate-700 dark:text-slate-300'
              }`}
              title={detailData.is_favorite ? t('history.unfavorite') : t('history.favorite')}
            >
              <Star
                className={`w-3.5 h-3.5 ${
                  detailData.is_favorite ? 'fill-amber-400 text-amber-500' : 'text-slate-400'
                }`}
              />
              <span>{detailData.is_favorite ? ui('已收藏', 'Favorited') : t('history.favorite')}</span>
            </button>

            <button
              onClick={() => handleCopy(detailData.url, 'url')}
              className="p-1.5 border border-slate-200 dark:border-slate-700 rounded hover:bg-slate-100 dark:hover:bg-slate-800 text-slate-700 dark:text-slate-300 transition cursor-pointer"
              aria-label={t('detail.copyUrl')}
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
                {detailData.title || ui('(无标题)', '(Untitled)')}
              </span>
              <button
                onClick={() => handleCopy(detailData.title, 'title')}
                aria-label={t('detail.copyTitle')}
                className="opacity-60 hover:opacity-100 p-0.5 cursor-pointer"
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
                <span>{ui('自定义标签', 'Tags')}</span>
              </span>
              {!isAddingTag && (
                <button
                  onClick={() => setIsAddingTag(true)}
                  className="flex items-center gap-1 text-[11px] text-blue-600 dark:text-blue-400 hover:underline cursor-pointer"
                >
                  <Plus className="w-3 h-3" />
                  <span>{ui('添加标签', 'Add tag')}</span>
                </button>
              )}
            </div>

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
                      aria-label={ui(`移除标签 ${tg}`, `Remove tag ${tg}`)}
                      className="text-blue-400 hover:text-blue-700 dark:hover:text-blue-200 cursor-pointer"
                    >
                      <X className="w-3 h-3" />
                    </button>
                  </span>
                ))
              ) : (
                <span className="text-[11px] text-slate-400">{ui('暂无标签，方便聚类与回顾', 'No tags yet')}</span>
              )}
            </div>

            {isAddingTag && (
              <form onSubmit={handleAddTagSubmit} className="flex items-center gap-1.5 pt-1">
                <input
                  type="text"
                  value={newTagInput}
                  onChange={(e) => setNewTagInput(e.target.value)}
                  placeholder={ui('标签名 (回车确认)', 'Tag name (Enter)')}
                  autoFocus
                  className="flex-1 px-2 py-1 rounded border border-slate-300 dark:border-slate-600 bg-white dark:bg-slate-900 text-xs focus:outline-none focus:border-blue-500"
                />
                <button
                  type="submit"
                  className="px-2 py-1 rounded bg-blue-600 text-white font-medium hover:bg-blue-700 transition cursor-pointer"
                >
                  {ui('保存', 'Save')}
                </button>
                <button
                  type="button"
                  onClick={() => {
                    setIsAddingTag(false);
                    setNewTagInput('');
                  }}
                  className="px-2 py-1 rounded border border-slate-300 dark:border-slate-600 text-slate-600 dark:text-slate-300 cursor-pointer"
                >
                  {t('export.cancel')}
                </button>
              </form>
            )}
          </div>

          {/* Visit Frequency & Lifecycle Stats */}
          <div className="grid grid-cols-2 gap-2">
            <div className="p-2.5 rounded bg-slate-50 dark:bg-slate-800/40 border border-slate-200 dark:border-slate-700/60 flex flex-col gap-1">
              <span className="text-slate-500 flex items-center gap-1 text-[11px]">
                <TrendingUp className="w-3.5 h-3.5 text-emerald-500" /> {ui('该网址累计访问', 'Total URL visits')}
              </span>
              <span className="text-base font-bold text-slate-800 dark:text-slate-100 font-mono">
                {detailData.total_url_visits || 1} <span className="text-xs font-normal text-slate-400">{t('domain.visits')}</span>
              </span>
            </div>
            <div className="p-2.5 rounded bg-slate-50 dark:bg-slate-800/40 border border-slate-200 dark:border-slate-700/60 flex flex-col gap-1">
              <span className="text-slate-500 flex items-center gap-1 text-[11px]">
                <Calendar className="w-3.5 h-3.5 text-blue-500" /> {ui('首次发现时间', 'First seen')}
              </span>
              <span className="text-[11px] text-slate-700 dark:text-slate-300 font-mono truncate" title={detailData.first_visit_time ? formatDateTime(detailData.first_visit_time) : '-'}>
                {detailData.first_visit_time ? new Date(detailData.first_visit_time).toLocaleDateString() : '-'}
              </span>
            </div>
          </div>

          {/* Link Health & Archive */}
          <div className="flex flex-col gap-2 p-2.5 rounded bg-slate-50 dark:bg-slate-800/40 border border-slate-200 dark:border-slate-700/60">
            <div className="flex items-center justify-between">
              <span className="font-semibold text-slate-700 dark:text-slate-200 flex items-center gap-1.5">
                <ShieldCheck className="w-3.5 h-3.5 text-blue-500" />
                <span>{ui('链接存活与本地阅读索引', 'Link health & reading index')}</span>
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
                <span>{isCheckingHealth ? ui('检测中...', 'Checking...') : ui('即时测活', 'Check now')}</span>
              </button>
            </div>

            <div className="flex items-center justify-between py-1.5 px-2 rounded bg-white dark:bg-slate-900 border border-slate-200/80 dark:border-slate-800">
              <span className="text-[11px] text-slate-500">{ui('存活可用性', 'Reachability')}</span>
              {linkHealth ? (
                linkHealth.is_alive ? (
                  <span className="inline-flex items-center gap-1 text-[11px] font-medium text-emerald-600 dark:text-emerald-400">
                    <ShieldCheck className="w-3.5 h-3.5" />
                    <span>{ui('正常在线', 'Online')} {linkHealth.status_code ? `(${linkHealth.status_code})` : ''}</span>
                  </span>
                ) : (
                  <span className="inline-flex items-center gap-1 text-[11px] font-medium text-rose-600 dark:text-rose-400">
                    <ShieldAlert className="w-3.5 h-3.5" />
                    <span>{ui('可能失效', 'Unreachable')} {linkHealth.status_code ? `(${linkHealth.status_code})` : ''}</span>
                  </span>
                )
              ) : (
                <span className="text-[11px] text-slate-400">{ui('尚未检测', 'Not checked')}</span>
              )}
            </div>

            {healthError && <p role="alert" className="text-xs text-rose-600 dark:text-rose-300">{healthError}</p>}
            <div className="flex items-center gap-2 pt-1">
              <button
                onClick={handleOpenWayback}
                className="flex-1 flex items-center justify-center gap-1.5 py-1.5 px-2 rounded border border-slate-300 dark:border-slate-700 bg-white dark:bg-slate-900 hover:bg-slate-50 dark:hover:bg-slate-800 text-slate-700 dark:text-slate-200 transition font-medium cursor-pointer"
                title={ui('在互联网档案馆 (Archive.org) 查阅历史快照', 'View snapshots on Internet Archive')}
              >
                <Archive className="w-3.5 h-3.5 text-amber-500" />
                <span>{ui('Wayback 快照', 'Wayback')}</span>
              </button>

              <button
                onClick={() => {
                  setSearch(detailData.title || detailData.domain);
                  setCurrentTab('ai');
                  closeDetail();
                }}
                className="flex items-center justify-center gap-1 py-1.5 px-2.5 rounded border border-blue-200 dark:border-blue-800 bg-blue-50/60 dark:bg-blue-950/40 hover:bg-blue-100 text-blue-700 dark:text-blue-300 transition font-medium cursor-pointer"
                title={ui('进入 AI 语义检索与相似网页', 'Find related pages in AI Assistant')}
              >
                <Sparkles className="w-3.5 h-3.5" />
                <span>{ui('语义检索', 'Semantic')}</span>
              </button>
            </div>

            <button
              onClick={() => setIsOfflineReaderOpen(true)}
              className="w-full flex items-center justify-center gap-2 py-2 px-3 rounded-xl border border-blue-200 dark:border-blue-800/80 bg-blue-50/70 dark:bg-blue-950/40 hover:bg-blue-100 dark:hover:bg-blue-900/60 text-blue-700 dark:text-blue-300 transition font-medium cursor-pointer text-xs"
              title={ui('查阅本地独立离线阅读索引与归档信息', 'Inspect local reading index & archived metadata')}
            >
              <BookOpen className="w-3.5 h-3.5 text-blue-500" />
              <span>{ui('本地阅读索引与记录', 'Local reading index & archive')}</span>
            </button>
          </div>

          {/* Context Nearby Visits Stream */}
          <div className="flex flex-col gap-2 p-2.5 rounded bg-slate-50 dark:bg-slate-800/40 border border-slate-200 dark:border-slate-700/60">
            <div className="flex items-center justify-between">
              <span className="font-semibold text-slate-700 dark:text-slate-200 flex items-center gap-1.5">
                <Compass className="w-3.5 h-3.5 text-blue-500" />
                <span>{ui('前后访问时序', 'Nearby visits')}</span>
              </span>
              <span className="text-[11px] text-slate-400">
                {detailData.nearby_visits ? detailData.nearby_visits.length : 0} {ui('条', 'items')}
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
                              {ui('当前记录', 'Current')}
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
                  {ui('暂无上下文前后浏览', 'No surrounding visits')}
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
                <Globe className="w-3.5 h-3.5" /> {t('history.columns.domain')}
              </span>
              <button
                onClick={() => handleFilterDomain(detailData.domain)}
                className="text-blue-600 dark:text-blue-400 hover:underline font-mono cursor-pointer"
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
              <span className="text-slate-500">{t('history.columns.duration')}</span>
              <span className="font-mono text-slate-700 dark:text-slate-300">
                {detailData.visit_duration > 0 ? formatDurationCompact(detailData.visit_duration, language) : ui('未记录', 'Not recorded')}
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

      {/* Offline Reader Modal */}
      {isOfflineReaderOpen && detailData && (
        <OfflineReaderModal
          isOpen={isOfflineReaderOpen}
          urlId={detailData.url_id}
          url={detailData.url}
          title={detailData.title || detailData.url}
          onClose={() => setIsOfflineReaderOpen(false)}
        />
      )}
    </aside>
  );
};
