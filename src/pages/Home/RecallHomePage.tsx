import React, { useState, useEffect } from 'react';
import {
  Search,
  Sparkles,
  Clock,
  ExternalLink,
  Layers,
  Calendar,
  ArrowRight,
  Brain,
  BarChart2,
  Compass,
  History,
} from 'lucide-react';
import { tauriApi } from '../../services/tauri';
import { useAppStore } from '../../stores/useAppStore';
import { useHistoryStore } from '../../stores/useHistoryStore';
import { formatDurationCompact, formatRelativeTimestamp } from '../../utils/formatters';
import {
  ResearchSession,
  TopicItem,
  OnThisDayResult,
} from '../../types';

export const RecallHomePage: React.FC = () => {
  const { setCurrentTab, language, t } = useAppStore();
  const ui = (zh: string, en: string) => (language === 'en-US' ? en : zh);
  const { setSearch } = useHistoryStore();

  const [query, setQuery] = useState('');
  const [todaySummary, setTodaySummary] = useState<{ visits: number; domains: number } | null>(null);
  const [sessions, setSessions] = useState<ResearchSession[]>([]);
  const [topics, setTopics] = useState<TopicItem[]>([]);
  const [onThisDay, setOnThisDay] = useState<OnThisDayResult | null>(null);
  const [isLoadingHome, setIsLoadingHome] = useState(true);
  const [homeError, setHomeError] = useState<string | null>(null);
  const [homeLoadAttempt, setHomeLoadAttempt] = useState(0);

  const hour = new Date().getHours();
  const greeting =
    hour >= 5 && hour < 12
      ? ui('早上好', 'Good morning')
      : hour >= 12 && hour < 14
      ? ui('中午好', 'Good afternoon')
      : hour >= 14 && hour < 19
      ? ui('下午好', 'Good afternoon')
      : ui('晚上好', 'Good evening');

  useEffect(() => {
    let isMounted = true;
    const loadHomeData = async () => {
      setIsLoadingHome(true);
      setHomeError(null);
      try {
        const now = new Date();
        const startOfDay = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
        const endOfDay = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1).getTime() - 1;

        const [analyticsRes, sessionsRes, topicsRes, onThisDayRes] = await Promise.allSettled([
          tauriApi.getAnalytics({ startTime: startOfDay, endTime: endOfDay, summaryOnly: true }),
          tauriApi.getResearchSessions(7, 4),
          tauriApi.getAllTopics(),
          tauriApi.getOnThisDay(),
        ]);

        if (!isMounted) return;

        const failedCount = [analyticsRes, sessionsRes, topicsRes, onThisDayRes].filter(
          (result) => result.status === 'rejected'
        ).length;
        if (failedCount === 4) {
          setHomeError(ui('首页数据暂时无法加载，请稍后重试', 'Home data failed to load; please retry'));
        } else if (failedCount > 0) {
          setHomeError(ui('部分首页数据暂时不可用', 'Some home widgets could not be loaded'));
        }

        if (analyticsRes.status === 'fulfilled') {
          const s = analyticsRes.value;
          setTodaySummary({
            visits: s.total_visits || 0,
            domains: s.unique_domains || 0,
          });
        }

        if (sessionsRes.status === 'fulfilled') {
          setSessions(sessionsRes.value || []);
        }

        if (topicsRes.status === 'fulfilled') {
          setTopics(topicsRes.value || []);
        }

        if (onThisDayRes.status === 'fulfilled') {
          setOnThisDay(onThisDayRes.value);
        }
      } catch (err) {
        console.error('Failed to load home recall data:', err);
        if (isMounted) setHomeError(ui('首页数据暂时无法加载，请稍后重试', 'Home data failed to load; please retry'));
      } finally {
        if (isMounted) setIsLoadingHome(false);
      }
    };

    loadHomeData();
    return () => {
      isMounted = false;
    };
  }, [homeLoadAttempt, language]);

  const handleSearchSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    const trimmed = query.trim();
    if (!trimmed) {
      window.dispatchEvent(new CustomEvent('browsory:open-quick-search'));
      return;
    }
    setSearch(trimmed);
    setCurrentTab('history');
  };

  const handleTriggerSpotlight = () => {
    window.dispatchEvent(new CustomEvent('browsory:open-quick-search'));
  };

  const handleJumpToHistory = (searchKeyword: string) => {
    setSearch(searchKeyword);
    setCurrentTab('history');
  };

  return (
    <div className="flex-1 h-full overflow-y-auto bg-slate-50/50 dark:bg-slate-900/60 p-8 select-none">
      <div className="max-w-4xl mx-auto space-y-8 animate-in fade-in duration-300">
        {/* Top Header & Today Pulse */}
        <div className="flex flex-col md:flex-row md:items-end justify-between gap-4 border-b border-slate-200/80 dark:border-slate-800 pb-6">
          <div>
            <div className="flex items-center gap-2 mb-1.5">
              <span className="inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full text-[11px] font-semibold bg-blue-50 dark:bg-blue-950/70 text-blue-600 dark:text-blue-400 border border-blue-200/60 dark:border-blue-800/60">
                <Brain className="w-3.5 h-3.5" />
                {t('nav.home')}
              </span>
              <span className="text-[11px] text-slate-400 dark:text-slate-500">
                {ui('本地加密存储', 'Encrypted local vault')}
              </span>
            </div>
            <h1 className="text-3xl font-extrabold tracking-tight text-slate-900 dark:text-slate-100">
              {greeting}
            </h1>
            <p className="text-sm text-slate-500 dark:text-slate-400 mt-1">
              {ui('重温与检索浏览历史记录', 'Search, revisit, and explore your local browsing archive')}
            </p>
          </div>

          {/* Today's Pulse Pill */}
          <div className="flex items-center gap-4 px-4 py-2.5 rounded-2xl bg-white dark:bg-slate-800/90 border border-slate-200/80 dark:border-slate-700/80 shadow-sm shrink-0">
            <div className="flex items-center gap-2">
              <Clock className="w-4 h-4 text-blue-500" />
              <div className="text-xs">
                <span className="font-bold text-slate-900 dark:text-slate-100">
                  {isLoadingHome ? '—' : (todaySummary?.visits ?? 0)}
                </span>
                <span className="text-slate-400 ml-1">{ui('次访问', 'visits today')}</span>
              </div>
            </div>
            <div className="h-4 w-px bg-slate-200 dark:bg-slate-700" />
            <div className="flex items-center gap-2">
              <Compass className="w-4 h-4 text-emerald-500" />
              <div className="text-xs">
                <span className="font-bold text-slate-900 dark:text-slate-100">
                  {isLoadingHome ? '—' : (todaySummary?.domains ?? 0)}
                </span>
                <span className="text-slate-400 ml-1">{ui('个站点', 'domains')}</span>
              </div>
            </div>
          </div>
        </div>

        {homeError && (
          <div className="flex items-center justify-between gap-3 px-3 py-2 border border-amber-200 bg-amber-50 text-amber-800 dark:border-amber-900/70 dark:bg-amber-950/30 dark:text-amber-300 text-xs rounded-lg" role="status">
            <span>{homeError}</span>
            <button type="button" onClick={() => setHomeLoadAttempt((attempt) => attempt + 1)} className="font-medium underline underline-offset-2 cursor-pointer">
              {t('common.retry')}
            </button>
          </div>
        )}

        {/* Central Spotlight Search Bar */}
        <div className="relative">
          <form onSubmit={handleSearchSubmit} className="relative flex items-center">
            <Search className="w-5 h-5 absolute left-4 text-slate-400 dark:text-slate-500" />
            <input
              type="text"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={ui(
                '搜索历史记录、网页标题或网址 (按 Enter 搜索，Ctrl+K 快捷检索)...',
                'Search history titles or URLs (Enter to search, Ctrl+K for Quick Search)...'
              )}
              className="w-full pl-12 pr-28 py-4 bg-white dark:bg-slate-800 border-2 border-slate-200/90 dark:border-slate-700/80 hover:border-blue-400/80 focus:border-blue-500 dark:focus:border-blue-500 rounded-2xl text-base text-slate-900 dark:text-slate-100 placeholder-slate-400 shadow-sm outline-none transition"
            />
            <button
              type="button"
              onClick={handleTriggerSpotlight}
              className="absolute right-3.5 flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg bg-slate-100 dark:bg-slate-700/80 hover:bg-slate-200 dark:hover:bg-slate-700 text-[11px] font-mono font-medium text-slate-600 dark:text-slate-300 border border-slate-200 dark:border-slate-600 transition cursor-pointer"
              title={ui('按 Ctrl+K 唤起全局快捷搜索', 'Press Ctrl+K to open Quick Search')}
            >
              <span>Ctrl K</span>
            </button>
          </form>
        </div>

        {/* Quick Access Portals */}
        <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
          <button
            type="button"
            onClick={() => setCurrentTab('history')}
            className="flex items-center justify-between p-4 rounded-2xl bg-white dark:bg-slate-800/80 border border-slate-200/80 dark:border-slate-800 hover:border-blue-400 dark:hover:border-blue-500 transition hover:shadow-md group text-left cursor-pointer"
          >
            <div className="flex items-center gap-3">
              <div className="w-10 h-10 rounded-xl bg-blue-50 dark:bg-blue-950/60 border border-blue-200/60 dark:border-blue-800/60 flex items-center justify-center text-blue-600 dark:text-blue-400 group-hover:scale-105 transition">
                <History className="w-5 h-5" />
              </div>
              <div>
                <h3 className="text-sm font-semibold text-slate-900 dark:text-slate-100">
                  {ui('浏览足迹时间线', 'History timeline')}
                </h3>
                <p className="text-xs text-slate-500 dark:text-slate-400">
                  {ui('查看全量历史与多源过滤', 'Browse archive & multi-browser filters')}
                </p>
              </div>
            </div>
            <ArrowRight className="w-4 h-4 text-slate-400 group-hover:text-blue-500 group-hover:translate-x-0.5 transition" />
          </button>

          <button
            type="button"
            onClick={() => setCurrentTab('analytics')}
            className="flex items-center justify-between p-4 rounded-2xl bg-white dark:bg-slate-800/80 border border-slate-200/80 dark:border-slate-800 hover:border-blue-400 dark:hover:border-blue-500 transition hover:shadow-md group text-left cursor-pointer"
          >
            <div className="flex items-center gap-3">
              <div className="w-10 h-10 rounded-xl bg-blue-50 dark:bg-blue-950/60 border border-blue-200/60 dark:border-blue-800/60 flex items-center justify-center text-blue-600 dark:text-blue-400 group-hover:scale-105 transition">
                <BarChart2 className="w-5 h-5" />
              </div>
              <div>
                <h3 className="text-sm font-semibold text-slate-900 dark:text-slate-100">
                  {ui('数据洞察大屏', 'Analytics dashboard')}
                </h3>
                <p className="text-xs text-slate-500 dark:text-slate-400">
                  {ui('热力图、域名趋势与习惯', 'Heatmaps, domain trends & habits')}
                </p>
              </div>
            </div>
            <ArrowRight className="w-4 h-4 text-slate-400 group-hover:text-blue-500 group-hover:translate-x-0.5 transition" />
          </button>

          <button
            type="button"
            onClick={() => setCurrentTab('ai')}
            className="flex items-center justify-between p-4 rounded-2xl bg-white dark:bg-slate-800/80 border border-slate-200/80 dark:border-slate-800 hover:border-blue-400 dark:hover:border-blue-500 transition hover:shadow-md group text-left cursor-pointer"
          >
            <div className="flex items-center gap-3">
              <div className="w-10 h-10 rounded-xl bg-blue-50 dark:bg-blue-950/60 border border-blue-200/60 dark:border-blue-800/60 flex items-center justify-center text-blue-600 dark:text-blue-400 group-hover:scale-105 transition">
                <Sparkles className="w-5 h-5" />
              </div>
              <div>
                <h3 className="text-sm font-semibold text-slate-900 dark:text-slate-100">
                  {ui('AI 历史问答', 'AI history assistant')}
                </h3>
                <p className="text-xs text-slate-500 dark:text-slate-400">
                  {ui('基于历史记录提问与溯源', 'Ask & retrieve with source citations')}
                </p>
              </div>
            </div>
            <ArrowRight className="w-4 h-4 text-slate-400 group-hover:text-blue-500 group-hover:translate-x-0.5 transition" />
          </button>
        </div>

        {/* Resume Research Sessions */}
        <div className="space-y-3">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <Layers className="w-4 h-4 text-blue-600 dark:text-blue-400" />
              <h2 className="text-sm font-bold text-slate-900 dark:text-slate-100 uppercase tracking-wider">
                {ui('继续之前的研究', 'Resume research sessions')}
              </h2>
            </div>
            <button
              type="button"
              onClick={() => setCurrentTab('analytics')}
              className="text-xs text-blue-600 dark:text-blue-400 hover:underline flex items-center gap-1 cursor-pointer"
            >
              <span>{ui('查看全部会话', 'View all sessions')}</span>
              <ArrowRight className="w-3 h-3" />
            </button>
          </div>

          {sessions.length === 0 ? (
            <div className="p-8 text-center rounded-2xl bg-white dark:bg-slate-800/60 border border-dashed border-slate-200 dark:border-slate-700/80 text-xs text-slate-400">
              {ui(
                '最近暂无集中的连续研究会话。随着日常浏览，系统会自动识别并归纳相关的专题浏览。',
                'No focused multi-page sessions detected recently. Sessions appear automatically as you research topics.'
              )}
            </div>
          ) : (
            <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
              {sessions.map((sess) => (
                <div
                  key={sess.session_id}
                  onClick={() => handleJumpToHistory(sess.dominant_domain)}
                  className="p-4 rounded-2xl bg-white dark:bg-slate-800/80 border border-slate-200/80 dark:border-slate-800 hover:border-blue-400/80 dark:hover:border-blue-500/80 transition hover:shadow-sm cursor-pointer flex flex-col justify-between gap-3 group"
                >
                  <div className="space-y-1.5 min-w-0">
                    <div className="flex items-center justify-between text-xs text-slate-400">
                      <span className="font-mono text-blue-600 dark:text-blue-400 font-semibold truncate max-w-[200px]">
                        {sess.dominant_domain}
                      </span>
                      <span>{formatRelativeTimestamp(sess.end_time, language, t('history.unknownTime'))}</span>
                    </div>
                    <p className="text-sm font-medium text-slate-800 dark:text-slate-200 line-clamp-2 group-hover:text-blue-600 dark:group-hover:text-blue-400 transition">
                      {sess.sample_title || sess.dominant_domain}
                    </p>
                  </div>

                  <div className="flex items-center justify-between text-xs text-slate-500 dark:text-slate-400 pt-2 border-t border-slate-100 dark:border-slate-800/60">
                    <div className="flex items-center gap-3">
                      <span>{ui(`${sess.visit_count} 个页面`, `${sess.visit_count} pages`)}</span>
                      <span>·</span>
                      <span>{ui(`时长 ${formatDurationCompact(sess.duration_secs, language)}`, `Duration ${formatDurationCompact(sess.duration_secs, language)}`)}</span>
                    </div>
                    <span className="text-[11px] text-blue-600 dark:text-blue-400 group-hover:translate-x-0.5 transition font-medium flex items-center gap-0.5">
                      {ui('查看记录', 'Inspect')} <ArrowRight className="w-3 h-3" />
                    </span>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>

        {/* Two-Column: Recent Topics Cloud & On This Day Flashcard */}
        <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
          <div className="space-y-3">
            <div className="flex items-center gap-2">
              <Brain className="w-4 h-4 text-emerald-600 dark:text-emerald-400" />
              <h2 className="text-sm font-bold text-slate-900 dark:text-slate-100 uppercase tracking-wider">
                {ui('关注知识主题', 'Focus topics')}
              </h2>
            </div>

            <div className="p-5 rounded-2xl bg-white dark:bg-slate-800/80 border border-slate-200/80 dark:border-slate-800 space-y-3">
              {topics.length === 0 ? (
                <p className="text-xs text-slate-400 text-center py-4">
                  {ui(
                    '暂无主题提取。浏览特定领域的内容后，系统会自动提炼关注主题。',
                    'No topics extracted yet. Browse more pages to build topic clusters.'
                  )}
                </p>
              ) : (
                <div className="flex flex-wrap gap-2">
                  {topics.slice(0, 14).map((topic) => (
                    <button
                      key={topic.id}
                      type="button"
                      onClick={() => handleJumpToHistory(topic.name)}
                      className="px-3 py-1.5 rounded-xl bg-slate-100 dark:bg-slate-700/60 hover:bg-emerald-50 dark:hover:bg-emerald-950/50 hover:text-emerald-600 dark:hover:text-emerald-400 text-xs font-medium text-slate-700 dark:text-slate-300 border border-slate-200/60 dark:border-slate-700 transition flex items-center gap-1.5 cursor-pointer"
                    >
                      <span>{topic.name}</span>
                      <span className="text-[11px] text-slate-400 font-mono">
                        {topic.category}
                      </span>
                    </button>
                  ))}
                </div>
              )}
            </div>
          </div>

          <div className="space-y-3">
            <div className="flex items-center gap-2">
              <Calendar className="w-4 h-4 text-amber-500" />
              <h2 className="text-sm font-bold text-slate-900 dark:text-slate-100 uppercase tracking-wider">
                {ui('那年今日', 'On this day')}
              </h2>
            </div>

            <div className="p-5 rounded-2xl bg-white dark:bg-slate-800/80 border border-slate-200/80 dark:border-slate-800 space-y-3">
              {onThisDay && onThisDay.items.length > 0 ? (
                <div className="space-y-3">
                  {onThisDay.items.slice(0, 3).map((item) => (
                    <div
                      key={item.id}
                      className="p-3 rounded-xl bg-slate-50 dark:bg-slate-900/60 border border-slate-100 dark:border-slate-800 flex items-start justify-between gap-3 group"
                    >
                      <div className="min-w-0 space-y-1">
                        <div className="flex items-center gap-2 text-[11px] text-slate-400">
                          <span className="px-1.5 py-0.5 rounded bg-amber-100 dark:bg-amber-950/70 text-amber-700 dark:text-amber-400 font-semibold text-[11px]">
                            {item.years_ago > 0
                              ? ui(`${item.years_ago} 年前的今天`, `${item.years_ago} yr(s) ago`)
                              : ui('历史记录', 'Archived')}
                          </span>
                          <span className="truncate">{item.domain}</span>
                        </div>
                        <h4
                          onClick={() => tauriApi.openExternalUrl(item.url)}
                          className="text-xs font-semibold text-slate-800 dark:text-slate-200 hover:text-blue-600 dark:hover:text-blue-400 cursor-pointer line-clamp-1"
                        >
                          {item.title || item.url}
                        </h4>
                      </div>

                      <div className="flex items-center gap-1 shrink-0 pt-0.5">
                        <button
                          type="button"
                          onClick={() => tauriApi.openExternalUrl(item.url)}
                          aria-label={t('detail.openInBrowser')}
                          className="p-1.5 rounded-lg hover:bg-slate-200/60 dark:hover:bg-slate-800 text-slate-400 hover:text-slate-700 dark:hover:text-slate-200 transition cursor-pointer"
                          title={t('detail.openInBrowser')}
                        >
                          <ExternalLink className="w-3.5 h-3.5" />
                        </button>
                      </div>
                    </div>
                  ))}
                </div>
              ) : (
                <div className="text-center py-6 space-y-1">
                  <p className="text-xs font-medium text-slate-600 dark:text-slate-300">
                    {ui('往年今日暂无浏览记录', 'No visits recorded on this date in prior years')}
                  </p>
                  <p className="text-xs text-slate-400">
                    {ui('持续积累浏览足迹后，系统会在每年同日呈现历史回顾。', 'Anniversary highlights appear here as your local archive spans multiple years.')}
                  </p>
                </div>
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};
