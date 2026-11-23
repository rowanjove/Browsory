import React, { useEffect, useState, useRef } from 'react';
import {
  Globe,
  Clock,
  Compass,
  TrendingUp,
  TrendingDown,
  Calendar,
  RefreshCw,
  Loader2,
  Search,
  Sparkles,
  ExternalLink,
  Layers,
  BarChart3,
  Flame,
  FileText,
  Repeat,
  History,
  Activity,
  ChevronRight,
  Brain,
} from 'lucide-react';
import { tauriApi } from '../../services/tauri';
import {
  AnalyticsSummary,
  InterestEvolution,
  ResearchSession,
  TopicItem,
  WebsiteRankingItem,
  DomainDynamics,
  OnThisDayResult,
} from '../../types';
import { useAppStore } from '../../stores/useAppStore';
import { useHistoryStore } from '../../stores/useHistoryStore';
import { DomainDetailModal } from './DomainDetailModal';
import { createRequestSequence } from '../../services/requestSequence';
import { formatDurationCompact } from '../../utils/formatters';

const PanelError: React.FC<{ message: string; onRetry: () => void; staleRange?: string }> = ({
  message,
  onRetry,
  staleRange,
}) => {
  const { t } = useAppStore();
  return (
    <div className="flex items-center justify-between gap-3 rounded border border-rose-200 bg-rose-50 px-3 py-2 text-[11px] text-rose-700 dark:border-rose-900/60 dark:bg-rose-950/30 dark:text-rose-300" role="alert">
      <span className="min-w-0">
        {message}
        {staleRange ? ` ${t('analytics.staleData', { range: staleRange })}` : ''}
      </span>
      <button type="button" onClick={onRetry} className="shrink-0 rounded border border-rose-300 px-2 py-1 font-medium hover:bg-rose-100 dark:border-rose-800 dark:hover:bg-rose-900/50">
        {t('common.retry')}
      </button>
    </div>
  );
};

export const AnalyticsPage: React.FC = () => {
  const { showToast, setCurrentTab, language } = useAppStore();
  const ui = (zh: string, en: string) => (language === 'en-US' ? en : zh);
  const { setSearch } = useHistoryStore();

  const [activeView, setActiveView] = useState<'overview' | 'interests' | 'sessions' | 'on_this_day'>('overview');
  const [days, setDays] = useState<number>(30); // Default 30 days
  const [rankingLimit, setRankingLimit] = useState<20 | 50 | 100>(20);

  const [data, setData] = useState<AnalyticsSummary | null>(null);
  const [ranking, setRanking] = useState<WebsiteRankingItem[]>([]);
  const [dynamics, setDynamics] = useState<DomainDynamics | null>(null);
  const [onThisDayResult, setOnThisDayResult] = useState<OnThisDayResult | null>(null);
  const [onThisDayDate, setOnThisDayDate] = useState<string>(() => {
    const today = new Date();
    const y = today.getFullYear();
    const m = String(today.getMonth() + 1).padStart(2, '0');
    const d = String(today.getDate()).padStart(2, '0');
    return `${y}-${m}-${d}`;
  });
  const [selectedYear, setSelectedYear] = useState<number | undefined>(undefined);
  const [currentPage, setCurrentPage] = useState<number>(1);
  const [isLoadingOnThisDay, setIsLoadingOnThisDay] = useState(false);
  const [sessions, setSessions] = useState<ResearchSession[]>([]);

  const [topics, setTopics] = useState<TopicItem[]>([]);
  const [evolution, setEvolution] = useState<InterestEvolution | null>(null);
  const [isLoadingInterests, setIsLoadingInterests] = useState(false);
  const [aiReport, setAiReport] = useState<string | null>(null);
  const [isGeneratingReport, setIsGeneratingReport] = useState(false);

  const [isLoading, setIsLoading] = useState(false);
  const [isLoadingRanking, setIsLoadingRanking] = useState(false);
  const [isLoadingSessions, setIsLoadingSessions] = useState(false);
  const [selectedDomain, setSelectedDomain] = useState<string | null>(null);
  const [analyticsError, setAnalyticsError] = useState<string | null>(null);
  const [rankingError, setRankingError] = useState<string | null>(null);
  const [interestsError, setInterestsError] = useState<string | null>(null);
  const [sessionsError, setSessionsError] = useState<string | null>(null);
  const [onThisDayError, setOnThisDayError] = useState<string | null>(null);
  const [analyticsDataDays, setAnalyticsDataDays] = useState<number | null>(null);
  const [rankingDataDays, setRankingDataDays] = useState<number | null>(null);
  const [interestsDataDays, setInterestsDataDays] = useState<number | null>(null);
  const [sessionsDataDays, setSessionsDataDays] = useState<number | null>(null);
  const [onThisDayDataKey, setOnThisDayDataKey] = useState<string | null>(null);

  const analyticsSequence = useRef(createRequestSequence());
  const rankingSequence = useRef(createRequestSequence());
  const interestsSequence = useRef(createRequestSequence());
  const sessionsSequence = useRef(createRequestSequence());
  const onThisDaySequence = useRef(createRequestSequence());

  const startTime = days > 0 ? Date.now() - days * 86400 * 1000 : undefined;
  const endTime = undefined;
  const formatRange = (value: number | null) => {
    if (value === null) return '';
    return value === 0 ? ui('全历史', 'All history') : ui(`最近 ${value} 天`, `Last ${value} days`);
  };

  const fetchAnalytics = async (selectedDays: number) => {
    const requestId = analyticsSequence.current.next();
    setIsLoading(true);
    setAnalyticsError(null);
    const start = selectedDays > 0 ? Date.now() - selectedDays * 86400 * 1000 : undefined;
    try {
      const [summary, dyn] = await Promise.all([
        tauriApi.getAnalytics(selectedDays === 0 ? undefined : selectedDays),
        tauriApi.getDomainDynamics(start),
      ]);
      if (!analyticsSequence.current.isCurrent(requestId)) return;
      setData(summary);
      setDynamics(dyn);
      setAnalyticsDataDays(selectedDays);
    } catch (err: any) {
      if (!analyticsSequence.current.isCurrent(requestId)) return;
      const message = err?.message || ui('获取分析统计失败', 'Failed to load analytics summary');
      setAnalyticsError(message);
      showToast(message, 'error');
    } finally {
      if (analyticsSequence.current.isCurrent(requestId)) setIsLoading(false);
    }
  };

  const fetchRanking = async (selectedDays: number, limit: number) => {
    const requestId = rankingSequence.current.next();
    setIsLoadingRanking(true);
    setRankingError(null);
    const start = selectedDays > 0 ? Date.now() - selectedDays * 86400 * 1000 : undefined;
    try {
      const list = await tauriApi.getWebsiteRanking(start, undefined, limit);
      if (!rankingSequence.current.isCurrent(requestId)) return;
      setRanking(list);
      setRankingDataDays(selectedDays);
    } catch (err: any) {
      if (!rankingSequence.current.isCurrent(requestId)) return;
      console.error('获取网站排名失败:', err);
      setRankingError(err?.message || ui('获取网站排名失败，请重试', 'Failed to load website ranking; please retry'));
    } finally {
      if (rankingSequence.current.isCurrent(requestId)) setIsLoadingRanking(false);
    }
  };

  const fetchInterests = async (selectedDays: number) => {
    const requestId = interestsSequence.current.next();
    setIsLoadingInterests(true);
    setInterestsError(null);
    try {
      const [tList, evo] = await Promise.all([
        tauriApi.getAllTopics(),
        tauriApi.getInterestEvolution(selectedDays === 0 ? 30 : selectedDays),
      ]);
      if (!interestsSequence.current.isCurrent(requestId)) return;
      setTopics(tList);
      setEvolution(evo);
      setInterestsDataDays(selectedDays);
    } catch (err: any) {
      if (!interestsSequence.current.isCurrent(requestId)) return;
      console.error('获取兴趣演化失败:', err);
      setInterestsError(err?.message || ui('获取兴趣演化失败，请重试', 'Failed to load interest evolution; please retry'));
    } finally {
      if (interestsSequence.current.isCurrent(requestId)) setIsLoadingInterests(false);
    }
  };

  const handleGeneratePeriodReport = async () => {
    setIsGeneratingReport(true);
    try {
      const report = await tauriApi.generateAiPeriodComparison(days === 0 ? 30 : days);
      setAiReport(report);
      showToast(ui('AI 认知演化报告生成完毕', 'AI evolution report generated'), 'success');
    } catch (err: any) {
      showToast(err?.message || ui('生成 AI 报告失败，请检查设置中的 AI 配置', 'Failed to generate AI report; check your AI settings'), 'error');
    } finally {
      setIsGeneratingReport(false);
    }
  };

  const fetchSessions = async (selectedDays: number) => {
    const requestId = sessionsSequence.current.next();
    setIsLoadingSessions(true);
    setSessionsError(null);
    try {
      const list = await tauriApi.getResearchSessions(selectedDays === 0 ? undefined : selectedDays, 50);
      if (!sessionsSequence.current.isCurrent(requestId)) return;
      setSessions(list);
      setSessionsDataDays(selectedDays);
    } catch (err: any) {
      if (!sessionsSequence.current.isCurrent(requestId)) return;
      setSessionsError(err?.message || ui('获取研究会话失败，请重试', 'Failed to load research sessions; please retry'));
      showToast(err?.message || ui('获取研究会话失败', 'Failed to load research sessions'), 'error');
    } finally {
      if (sessionsSequence.current.isCurrent(requestId)) setIsLoadingSessions(false);
    }
  };

  const fetchOnThisDayData = async (targetDate?: string, year?: number, page: number = 1) => {
    const requestId = onThisDaySequence.current.next();
    setIsLoadingOnThisDay(true);
    setOnThisDayError(null);
    try {
      const result = await tauriApi.getOnThisDay(targetDate, year, page, 30);
      if (!onThisDaySequence.current.isCurrent(requestId)) return;
      setOnThisDayResult(result);
      setOnThisDayDataKey(`${targetDate ?? onThisDayDate}:${year ?? 'all'}:${page}`);
    } catch (err: any) {
      if (!onThisDaySequence.current.isCurrent(requestId)) return;
      console.error('获取历史上的今天失败:', err);
      setOnThisDayError(err?.message || ui('获取历史上的今天失败，请重试', 'Failed to load On This Day; please retry'));
    } finally {
      if (onThisDaySequence.current.isCurrent(requestId)) setIsLoadingOnThisDay(false);
    }
  };

  useEffect(() => {
    fetchAnalytics(days);
  }, [days]);

  useEffect(() => {
    fetchRanking(days, rankingLimit);
  }, [days, rankingLimit]);

  useEffect(() => {
    if (activeView === 'interests') {
      fetchInterests(days);
    } else if (activeView === 'sessions') {
      fetchSessions(days);
    } else if (activeView === 'on_this_day') {
      fetchOnThisDayData(onThisDayDate, selectedYear, currentPage);
    }
  }, [activeView, days, onThisDayDate, selectedYear, currentPage]);

  const handleSearchClick = (keyword: string) => {
    setSearch(keyword);
    setCurrentTab('history');
  };

  const trendScrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (trendScrollRef.current && (data?.daily_trend.length ?? 0) > 40) {
      trendScrollRef.current.scrollLeft = trendScrollRef.current.scrollWidth;
    }
  }, [data?.daily_trend]);

  // Calculations for charts
  const maxDailyCount = data?.daily_trend?.reduce((max, d) => Math.max(max, d.count), 1) ?? 1;
  const maxHourlyCount = Math.max(...(data?.hourly_distribution.map((h) => h.count) || [1]), 1);
  const maxHeatmapCount = Math.max(...(data?.weekly_heatmap.map((p) => p.count) || [1]), 1);

  const getHeatmapColor = (count: number) => {
    if (count === 0) return 'bg-slate-100 dark:bg-slate-800/80';
    const ratio = count / maxHeatmapCount;
    if (ratio < 0.25) return 'bg-emerald-200 dark:bg-emerald-950/80';
    if (ratio < 0.5) return 'bg-emerald-400 dark:bg-emerald-800';
    if (ratio < 0.75) return 'bg-emerald-500 dark:bg-emerald-600';
    return 'bg-emerald-600 dark:bg-emerald-500';
  };

  const dayNames =
    language === 'en-US'
      ? ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat']
      : ['周日', '周一', '周二', '周三', '周四', '周五', '周六'];

  return (
    <div className="flex-1 flex flex-col h-full bg-slate-50 dark:bg-slate-900 overflow-y-auto p-6 text-xs select-none">
      {/* Header with time scope selector & View Tabs */}
      <div className="flex flex-wrap items-center justify-between gap-3 pb-4 border-b border-slate-200 dark:border-slate-800">
        <div className="flex flex-col gap-1">
          <div className="flex flex-wrap items-center gap-3">
            <h1 className="text-base font-semibold text-slate-800 dark:text-slate-100 flex items-center gap-2">
              <TrendingUp className="w-4 h-4 text-blue-600 dark:text-blue-400" />
              <span>{ui('统计分析', 'Analytics')}</span>
            </h1>

            {/* View Switcher */}
            <div className="flex items-center bg-slate-200/80 dark:bg-slate-800 p-0.5 rounded-lg text-xs">
              <button
                onClick={() => setActiveView('overview')}
                className={`flex items-center gap-1.5 px-3 py-1 rounded-md transition font-medium ${
                  activeView === 'overview'
                    ? 'bg-white dark:bg-slate-700 text-blue-600 dark:text-blue-400 shadow-sm'
                    : 'text-slate-600 dark:text-slate-400 hover:text-slate-900 dark:hover:text-slate-200'
                }`}
              >
                <BarChart3 className="w-3.5 h-3.5" />
                <span>{ui('概览统计', 'Overview')}</span>
              </button>
              <button
                onClick={() => setActiveView('interests')}
                className={`flex items-center gap-1.5 px-3 py-1 rounded-md transition font-medium ${
                  activeView === 'interests'
                    ? 'bg-white dark:bg-slate-700 text-blue-600 dark:text-blue-400 shadow-sm'
                    : 'text-slate-600 dark:text-slate-400 hover:text-slate-900 dark:hover:text-slate-200'
                }`}
              >
                <Brain className="w-3.5 h-3.5" />
                <span>{ui('兴趣演化', 'Interests')}</span>
              </button>
              <button
                onClick={() => setActiveView('sessions')}
                className={`flex items-center gap-1.5 px-3 py-1 rounded-md transition font-medium ${
                  activeView === 'sessions'
                    ? 'bg-white dark:bg-slate-700 text-blue-600 dark:text-blue-400 shadow-sm'
                    : 'text-slate-600 dark:text-slate-400 hover:text-slate-900 dark:hover:text-slate-200'
                }`}
              >
                <Layers className="w-3.5 h-3.5" />
                <span>{ui('研究会话', 'Sessions')}</span>
              </button>
              <button
                onClick={() => setActiveView('on_this_day')}
                className={`flex items-center gap-1.5 px-3 py-1 rounded-md transition font-medium ${
                  activeView === 'on_this_day'
                    ? 'bg-white dark:bg-slate-700 text-blue-600 dark:text-blue-400 shadow-sm'
                    : 'text-slate-600 dark:text-slate-400 hover:text-slate-900 dark:hover:text-slate-200'
                }`}
              >
                <Calendar className="w-3.5 h-3.5" />
                <span>{ui('那年今日', 'On this day')}</span>
              </button>
            </div>
          </div>
          <p className="text-slate-500">
            {ui(
              `本地统计 · 当前区间累计 ${data?.total_visits.toLocaleString() ?? '...'} 次访问`,
              `Local analytics · ${data?.total_visits.toLocaleString() ?? '...'} visits in range`,
            )}
          </p>
        </div>

        <div className="flex items-center gap-2">
          <div className="flex items-center gap-1 border border-slate-300 dark:border-slate-700 rounded px-2 py-1 bg-white dark:bg-slate-800">
            <Calendar className="w-3.5 h-3.5 text-slate-400" />
            <select
              value={days}
              onChange={(e) => setDays(Number(e.target.value))}
              aria-label={ui('统计时间范围', 'Analytics time range')}
              className="bg-transparent text-slate-700 dark:text-slate-200 focus:outline-none cursor-pointer text-xs"
            >
              <option value={7}>{ui('最近 7 天', 'Last 7 days')}</option>
              <option value={30}>{ui('最近 30 天', 'Last 30 days')}</option>
              <option value={90}>{ui('最近 90 天', 'Last 90 days')}</option>
              <option value={365}>{ui('最近 365 天', 'Last 365 days')}</option>
              <option value={0}>{ui('全量历史记录', 'All history')}</option>
            </select>
          </div>

          <button
            onClick={() => {
              fetchAnalytics(days);
              fetchRanking(days, rankingLimit);
              if (activeView === 'sessions') fetchSessions(days);
            }}
            disabled={isLoading || isLoadingSessions}
            className="p-1.5 rounded border border-slate-300 dark:border-slate-700 bg-white dark:bg-slate-800 hover:bg-slate-100 dark:hover:bg-slate-700 text-slate-600 dark:text-slate-300 transition"
            title={ui('刷新统计', 'Refresh analytics')}
          >
            <RefreshCw className={`w-3.5 h-3.5 ${isLoading || isLoadingSessions ? 'animate-spin' : ''}`} />
          </button>
        </div>
      </div>

      {isLoading && !data ? (
        <div className="flex-1 flex items-center justify-center py-24 text-slate-400 gap-2">
          <Loader2 className="w-4 h-4 animate-spin text-blue-600 dark:text-blue-400" />
          <span>{ui('正在加载统计数据...', 'Loading analytics...')}</span>
        </div>
      ) : analyticsError && !data ? (
        <div className="mt-5">
          <PanelError message={analyticsError} onRetry={() => fetchAnalytics(days)} />
        </div>
      ) : activeView === 'overview' ? (
        <div className="flex flex-col gap-5 mt-5">
          {analyticsError && (
            <PanelError
              message={analyticsError}
              staleRange={formatRange(analyticsDataDays)}
              onRetry={() => fetchAnalytics(days)}
            />
          )}
          {/* 1. Extended KPI Cards (5 Metrics) */}
          <div className="grid grid-cols-2 md:grid-cols-5 gap-3">
            {/* Total Visits with Period Growth */}
            <div className="bg-white dark:bg-slate-800 p-3.5 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-1.5">
              <div className="flex items-center justify-between text-slate-400">
                <span className="text-[11px] font-medium">{ui('总访问量', 'Total visits')}</span>
                <Activity className="w-3.5 h-3.5 text-blue-600 dark:text-blue-400" />
              </div>
              <div className="text-xl font-bold font-mono text-slate-800 dark:text-slate-100">
                {data?.total_visits.toLocaleString() ?? 0}
              </div>
              <div className="flex items-center gap-1.5 text-[10px]">
                {data?.period_comparison && days > 0 ? (
                  <>
                    <span
                      className={`inline-flex items-center gap-0.5 font-semibold px-1 py-0.2 rounded ${
                        data.period_comparison.growth_rate >= 0
                          ? 'bg-emerald-50 text-emerald-600 dark:bg-emerald-950/60 dark:text-emerald-400'
                          : 'bg-rose-50 text-rose-600 dark:bg-rose-950/60 dark:text-rose-400'
                      }`}
                    >
                      {data.period_comparison.growth_rate >= 0 ? (
                        <TrendingUp className="w-3 h-3" />
                      ) : (
                        <TrendingDown className="w-3 h-3" />
                      )}
                      <span>
                        {data.period_comparison.growth_rate >= 0 ? '+' : ''}
                        {data.period_comparison.growth_rate}%
                      </span>
                    </span>
                    <span className="text-slate-400">{ui('环比上一周期', 'vs previous period')}</span>
                  </>
                ) : (
                  <span className="text-slate-400">{ui('全历史累计', 'All-time cumulative')}</span>
                )}
              </div>
            </div>

            {/* Unique URLs */}
            <div className="bg-white dark:bg-slate-800 p-3.5 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-1.5">
              <div className="flex items-center justify-between text-slate-400">
                <span className="text-[11px] font-medium">{ui('独立网页数', 'Unique pages')}</span>
                <FileText className="w-3.5 h-3.5 text-blue-600 dark:text-blue-400" />
              </div>
              <div className="text-xl font-bold font-mono text-slate-800 dark:text-slate-100">
                {data?.unique_urls.toLocaleString() ?? 0}
              </div>
              <div className="text-[10px] text-slate-400">
                {ui('均访 ', 'Avg ')}
                {data && data.unique_urls > 0 ? (data.total_visits / data.unique_urls).toFixed(1) : 1}
                {ui(' 次/页', ' visits/page')}
              </div>
            </div>

            {/* Unique Domains */}
            <div className="bg-white dark:bg-slate-800 p-3.5 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-1.5">
              <div className="flex items-center justify-between text-slate-400">
                <span className="text-[11px] font-medium">{ui('独立域名数', 'Unique domains')}</span>
                <Globe className="w-3.5 h-3.5 text-cyan-500" />
              </div>
              <div className="text-xl font-bold font-mono text-slate-800 dark:text-slate-100">
                {data?.unique_domains.toLocaleString() ?? 0}
              </div>
              <div className="text-[10px] text-slate-400">
                {ui('涉及站点总广度', 'Distinct sites visited')}
              </div>
            </div>

            {/* Active Days */}
            <div className="bg-white dark:bg-slate-800 p-3.5 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-1.5">
              <div className="flex items-center justify-between text-slate-400">
                <span className="text-[11px] font-medium">{ui('活跃天数', 'Active days')}</span>
                <Calendar className="w-3.5 h-3.5 text-amber-500" />
              </div>
              <div className="text-xl font-bold font-mono text-slate-800 dark:text-slate-100">
                {data?.active_days ?? 0}{' '}
                <span className="text-xs font-normal text-slate-400">{ui('天', 'days')}</span>
              </div>
              <div className="text-[10px] text-slate-400">
                {days > 0
                  ? ui(
                      `活跃率 ${Math.min(Math.round(((data?.active_days || 0) / days) * 100), 100)}%`,
                      `${Math.min(Math.round(((data?.active_days || 0) / days) * 100), 100)}% active rate`,
                    )
                  : ui('持续活跃', 'All-time active days')}
              </div>
            </div>

            {/* Revisit Rate */}
            <div className="bg-white dark:bg-slate-800 p-3.5 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-1.5">
              <div className="flex items-center justify-between text-slate-400">
                <span className="text-[11px] font-medium">{ui('网页重访率', 'Revisit rate')}</span>
                <Repeat className="w-3.5 h-3.5 text-emerald-500" />
              </div>
              <div className="text-xl font-bold font-mono text-emerald-600 dark:text-emerald-400">
                {data?.revisit_rate ?? 0}%
              </div>
              <div className="text-[10px] text-slate-400">
                {ui('反复查阅的沉淀知识', 'Pages revisited more than once')}
              </div>
            </div>
          </div>

          {/* 2. Deep Website Analysis & Ranking (Top 20/50/100) */}
          <div className="bg-white dark:bg-slate-800 p-4 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-3">
            <div className="flex items-center justify-between pb-2 border-b border-slate-100 dark:border-slate-700">
              <div className="flex items-center gap-2">
                <Globe className="w-4 h-4 text-blue-600 dark:text-blue-400" />
                <span className="font-semibold text-slate-800 dark:text-slate-100">
                  {ui('网站访问排行', 'Top websites')}
                </span>
                <span className="text-slate-400 text-[11px]">
                  {ui('· 支持点击站点查看路径结构与时段分布', '· Click a domain to inspect paths and hours')}
                </span>
              </div>

              {/* Limit Switcher */}
              <div className="flex items-center bg-slate-100 dark:bg-slate-700/60 p-0.5 rounded text-[11px]">
                {([20, 50, 100] as const).map((lim) => (
                  <button
                    key={lim}
                    onClick={() => setRankingLimit(lim)}
                    className={`px-2.5 py-0.5 rounded font-medium transition ${
                      rankingLimit === lim
                        ? 'bg-white dark:bg-slate-800 text-blue-600 dark:text-blue-400 shadow-xs'
                        : 'text-slate-500 hover:text-slate-800 dark:hover:text-slate-200'
                    }`}
                  >
                    Top {lim}
                  </button>
                ))}
              </div>
            </div>

            {isLoadingRanking ? (
              <div className="py-12 flex items-center justify-center text-slate-400 gap-2">
                <Loader2 className="w-4 h-4 animate-spin text-blue-600 dark:text-blue-400" />
                <span>{ui('正在计算站点深度排行榜...', 'Computing website ranking...')}</span>
              </div>
            ) : rankingError && ranking.length === 0 ? (
              <PanelError message={rankingError} onRetry={() => fetchRanking(days, rankingLimit)} />
            ) : ranking.length === 0 ? (
              <div className="py-8 text-center text-slate-400">{ui('所选时段暂无网站数据', 'No website data in selected range')}</div>
            ) : (
              <>
                {rankingError && (
                  <PanelError
                    message={rankingError}
                    staleRange={formatRange(rankingDataDays)}
                    onRetry={() => fetchRanking(days, rankingLimit)}
                  />
                )}
                <div className="overflow-x-auto">
                <table className="w-full text-left border-collapse">
                  <thead>
                    <tr className="border-b border-slate-100 dark:border-slate-700/60 text-slate-400 text-[11px]">
                      <th className="pb-2 font-medium w-12 text-center">{ui('排名', 'Rank')}</th>
                      <th className="pb-2 font-medium">{ui('域名 / 站点', 'Domain')}</th>
                      <th className="pb-2 font-medium text-right">{ui('访问次数', 'Visits')}</th>
                      <th className="pb-2 font-medium text-right">{ui('涉及页面数', 'Pages')}</th>
                      <th className="pb-2 font-medium text-right">{ui('重访率', 'Revisit')}</th>
                      <th className="pb-2 font-medium text-right">{ui('活跃天数', 'Active days')}</th>
                      <th className="pb-2 font-medium text-right">{ui('最近访问', 'Last visit')}</th>
                      <th className="pb-2 font-medium text-center w-24">{ui('操作', 'Actions')}</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-slate-100 dark:divide-slate-700/50">
                    {ranking.map((item, idx) => {
                      const maxVisits = ranking[0]?.visits || 1;
                      const barPercent = Math.max(Math.round((item.visits / maxVisits) * 100), 4);
                      return (
                        <tr
                          key={item.domain}
                          className="hover:bg-slate-50/80 dark:hover:bg-slate-700/40 transition group cursor-pointer"
                          onClick={() => setSelectedDomain(item.domain)}
                        >
                          <td className="py-2.5 text-center font-mono text-slate-400 font-medium">
                            {idx + 1}
                          </td>
                          <td className="py-2.5 pr-3">
                            <div className="flex flex-col gap-1 max-w-[280px]">
                              <div className="flex items-center gap-1.5">
                                <span className="font-semibold text-slate-800 dark:text-slate-200 group-hover:text-blue-600 dark:group-hover:text-blue-400 truncate">
                                  {item.domain}
                                </span>
                              </div>
                              {/* Mini progress bar */}
                              <div className="w-36 h-1 bg-slate-100 dark:bg-slate-700 rounded-full overflow-hidden">
                                <div
                                  className="h-full bg-blue-500/80 group-hover:bg-blue-600 rounded-full transition-all"
                                  style={{ width: `${barPercent}%` }}
                                />
                              </div>
                            </div>
                          </td>
                          <td className="py-2.5 text-right font-mono font-bold text-slate-700 dark:text-slate-200">
                            {item.visits.toLocaleString()}
                          </td>
                          <td className="py-2.5 text-right font-mono text-slate-500 dark:text-slate-400">
                            {item.unique_urls.toLocaleString()}
                          </td>
                          <td className="py-2.5 text-right font-mono text-emerald-600 dark:text-emerald-400 font-medium">
                            {item.revisit_rate}%
                          </td>
                          <td className="py-2.5 text-right font-mono text-slate-500 dark:text-slate-400">
                            {ui(`${item.active_days} 天`, `${item.active_days}d`)}
                          </td>
                          <td className="py-2.5 text-right font-mono text-slate-400 text-[11px]">
                            {new Date(item.last_visit_time).toLocaleDateString()}
                          </td>
                          <td
                            className="py-2.5 text-center"
                            onClick={(e) => e.stopPropagation()}
                          >
                            <div className="flex items-center justify-center gap-1">
                              <button
                                onClick={() => setSelectedDomain(item.domain)}
                                className="px-2 py-1 rounded bg-blue-50 hover:bg-blue-100 dark:bg-blue-950/60 dark:hover:bg-blue-900 text-blue-600 dark:text-blue-400 text-[10px] font-medium transition"
                              >
                                {ui('查看详情', 'Details')}
                              </button>
                              <button
                                onClick={() => handleSearchClick(item.domain)}
                                className="p-1 rounded text-slate-400 hover:text-blue-600 hover:bg-slate-100 dark:hover:bg-slate-700 transition"
                                title={ui('在历史记录中检索', 'Search in history')}
                              >
                                <Search className="w-3 h-3" />
                              </button>
                            </div>
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
                </div>
              </>
            )}
          </div>

          {/* 4. Domain Dynamics Discovery */}
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            {/* New Domains */}
            <div className="bg-white dark:bg-slate-800 p-4 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-3">
              <div className="flex items-center justify-between pb-2 border-b border-slate-100 dark:border-slate-700">
                <div className="flex items-center gap-2 font-semibold text-slate-800 dark:text-slate-100">
                  <Sparkles className="w-4 h-4 text-emerald-500" />
                  <span>{ui('近期新增站点', 'Newly discovered domains')}</span>
                </div>
                <span className="text-[11px] text-emerald-600 dark:text-emerald-400 font-mono">
                  {ui(`${dynamics?.new_domains.length ?? 0} 个站点`, `${dynamics?.new_domains.length ?? 0} domains`)}
                </span>
              </div>

              <div className="flex flex-col gap-1.5 max-h-72 overflow-y-auto pr-1">
                {!dynamics?.new_domains || dynamics.new_domains.length === 0 ? (
                  <span className="text-slate-400 py-6 text-center">{ui('本时段暂无全新探索站点', 'No newly visited domains in this range')}</span>
                ) : (
                  dynamics.new_domains.map((nd) => (
                    <div
                      key={nd.domain}
                      onClick={() => setSelectedDomain(nd.domain)}
                      className="p-2 rounded hover:bg-slate-50 dark:hover:bg-slate-700/50 border border-slate-100 dark:border-slate-800 transition cursor-pointer flex items-center justify-between group"
                    >
                      <div className="flex flex-col gap-0.5 truncate pr-2">
                        <div className="flex items-center gap-2">
                          <span className="font-semibold text-slate-800 dark:text-slate-200 group-hover:text-blue-600 dark:group-hover:text-blue-400 truncate">
                            {nd.domain}
                          </span>
                          <span className="px-1.5 py-0.2 rounded text-[9px] bg-emerald-50 text-emerald-600 dark:bg-emerald-950/60 dark:text-emerald-300">
                            {ui('首次访问', 'First visit')}
                          </span>
                        </div>
                        <span className="text-[11px] text-slate-500 dark:text-slate-400 truncate">
                          {nd.sample_title || nd.sample_url}
                        </span>
                      </div>
                      <div className="flex flex-col items-end gap-0.5 shrink-0">
                        <span className="font-mono text-emerald-600 dark:text-emerald-400 font-bold">
                          {ui(`${nd.visits_in_period} 次`, `${nd.visits_in_period} visits`)}
                        </span>
                        <span className="text-[10px] text-slate-400 font-mono">
                          {new Date(nd.first_visit_time).toLocaleDateString()}
                        </span>
                      </div>
                    </div>
                  ))
                )}
              </div>
            </div>

            {/* Dormant Domains */}
            <div className="bg-white dark:bg-slate-800 p-4 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-3">
              <div className="flex items-center justify-between pb-2 border-b border-slate-100 dark:border-slate-700">
                <div className="flex items-center gap-2 font-semibold text-slate-800 dark:text-slate-100">
                  <History className="w-4 h-4 text-slate-400" />
                  <span>{ui('高频沉寂站点', 'Dormant frequent domains')}</span>
                </div>
                <span className="text-[11px] text-slate-400 font-mono">
                  {ui(`${dynamics?.dormant_domains.length ?? 0} 个站点`, `${dynamics?.dormant_domains.length ?? 0} domains`)}
                </span>
              </div>

              <div className="flex flex-col gap-1.5 max-h-72 overflow-y-auto pr-1">
                {!dynamics?.dormant_domains || dynamics.dormant_domains.length === 0 ? (
                  <span className="text-slate-400 py-6 text-center">{ui('暂无显著沉寂的高频站点', 'No dormant high-frequency domains')}</span>
                ) : (
                  dynamics.dormant_domains.map((dd) => (
                    <div
                      key={dd.domain}
                      onClick={() => handleSearchClick(dd.domain)}
                      className="p-2 rounded hover:bg-slate-50 dark:hover:bg-slate-700/50 border border-slate-100 dark:border-slate-800 transition cursor-pointer flex items-center justify-between group"
                    >
                      <div className="flex flex-col gap-0.5 truncate pr-2">
                        <span className="font-semibold text-slate-700 dark:text-slate-300 group-hover:text-blue-600 dark:group-hover:text-blue-400 truncate">
                          {dd.domain}
                        </span>
                        <span className="text-[10px] text-slate-400 font-mono">
                          {ui(
                            `历史累计 ${dd.total_historical_visits} 次访问 · 上次访问于 ${new Date(dd.last_visit_time).toLocaleDateString()}`,
                            `${dd.total_historical_visits} total visits · last visited ${new Date(dd.last_visit_time).toLocaleDateString()}`,
                          )}
                        </span>
                      </div>
                      <div className="flex items-center gap-2 shrink-0">
                        <span className="px-2 py-0.5 rounded bg-slate-100 dark:bg-slate-700 text-slate-500 dark:text-slate-400 text-[10px] font-mono">
                          {ui(`沉寂 ${dd.days_dormant} 天`, `Idle ${dd.days_dormant}d`)}
                        </span>
                        <ChevronRight className="w-3.5 h-3.5 text-slate-300 group-hover:text-slate-500" />
                      </div>
                    </div>
                  ))
                )}
              </div>
            </div>
          </div>

          {/* 5. Daily Trend Histogram/Chart */}
          <div className="bg-white dark:bg-slate-800 p-4 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-3 overflow-hidden">
            <div className="flex items-center justify-between font-medium text-slate-800 dark:text-slate-200">
              <div className="flex items-center gap-2">
                <TrendingUp className="w-4 h-4 text-emerald-500" />
                <span>{ui('每日访问趋势', 'Daily visits trend')}</span>
              </div>
              <div className="flex items-center gap-2">
                {data && data.daily_trend.length > 40 && (
                  <span className="text-[10px] text-slate-400 dark:text-slate-500 bg-slate-100 dark:bg-slate-700/60 px-2 py-0.5 rounded-full font-mono">
                    {ui('支持横向滑动浏览', 'Scroll horizontally')}
                  </span>
                )}
                <span className="text-slate-400 text-[11px] font-mono">
                  {ui(`${data?.daily_trend.length ?? 0} 天记录`, `${data?.daily_trend.length ?? 0} days`)}
                </span>
              </div>
            </div>

            {!data?.daily_trend || data.daily_trend.length === 0 ? (
              <span className="text-slate-400 py-8 text-center">{ui('所选时段暂无访问记录', 'No visits in selected range')}</span>
            ) : (
              <div
                ref={trendScrollRef}
                className="w-full overflow-x-auto history-scrollbar pt-7 pb-1.5"
              >
                <div
                  className="flex flex-col min-w-full"
                  style={{
                    width: data.daily_trend.length > 40 ? `${data.daily_trend.length * 16 + 28}px` : '100%',
                  }}
                >
                  <div className="h-32 flex items-end gap-1 pl-2 pr-6 relative">
                    {data.daily_trend.map((d, i) => {
                      const heightPercent = Math.max(Math.round((d.count / maxDailyCount) * 100), 4);
                      const isNearRight = i >= data.daily_trend.length - 2;
                      const isNearLeft = i < 2;
                      const tooltipPosClass = isNearRight
                        ? 'right-0 translate-x-0'
                        : isNearLeft
                        ? 'left-0 translate-x-0'
                        : 'left-1/2 -translate-x-1/2';
                      return (
                        <div
                          key={d.date}
                          className="flex-1 min-w-[10px] max-w-[32px] h-full flex items-end justify-center group relative"
                        >
                          {/* Tooltip */}
                          <div
                            className={`hidden group-hover:flex absolute bottom-[calc(100%+4px)] ${tooltipPosClass} pointer-events-none z-30 bg-slate-900/95 text-white text-[10px] px-2 py-0.5 rounded shadow-lg whitespace-nowrap font-mono`}
                          >
                            {d.date}: {ui(`${d.count} 次`, `${d.count} visits`)}
                          </div>
                          {/* Bar */}
                          <div
                            className="w-full bg-emerald-500 hover:bg-emerald-600 dark:bg-emerald-600 dark:hover:bg-emerald-500 rounded-t transition-all cursor-pointer min-h-[4px]"
                            style={{ height: `${heightPercent}%` }}
                          />
                        </div>
                      );
                    })}
                  </div>
                  {/* Dedicated X-axis Date Row */}
                  <div className="h-5 pt-1 border-t border-slate-200 dark:border-slate-700/80 flex items-center gap-1 pl-2 pr-6">
                    {data.daily_trend.map((d, i) => {
                      const step = data.daily_trend.length > 40
                        ? 7
                        : Math.max(Math.floor((data.daily_trend.length || 1) / 8), 1);
                      const showLabel = i % step === 0 || i === data.daily_trend.length - 1;
                      return (
                        <div
                          key={d.date}
                          className="flex-1 min-w-[10px] max-w-[32px] text-center overflow-visible"
                        >
                          {showLabel && (
                            <span
                              className={`text-[9px] text-slate-400 dark:text-slate-500 font-mono block whitespace-nowrap ${
                                i === data.daily_trend.length - 1 ? '-ml-3' : '-ml-2'
                              }`}
                            >
                              {d.date.length > 5 ? d.date.slice(5) : d.date}
                            </span>
                          )}
                        </div>
                      );
                    })}
                  </div>
                </div>
              </div>
            )}
          </div>

          {/* 6. 7x24 Weekly-Hourly Heatmap */}
          <div className="bg-white dark:bg-slate-800 p-4 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-3">
            <div className="flex items-center justify-between font-medium text-slate-800 dark:text-slate-200">
              <div className="flex items-center gap-2">
                <Flame className="w-4 h-4 text-orange-500" />
                <span>{ui('24小时时段访问热力图', 'Weekly hourly heatmap')}</span>
              </div>
              <div className="flex items-center gap-1 text-[11px] text-slate-400">
                <span>{ui('低', 'Low')}</span>
                <span className="w-2.5 h-2.5 rounded-sm bg-slate-100 dark:bg-slate-800 border border-slate-300 dark:border-slate-700"></span>
                <span className="w-2.5 h-2.5 rounded-sm bg-emerald-200 dark:bg-emerald-950"></span>
                <span className="w-2.5 h-2.5 rounded-sm bg-emerald-400 dark:bg-emerald-800"></span>
                <span className="w-2.5 h-2.5 rounded-sm bg-emerald-600 dark:bg-emerald-500"></span>
                <span>{ui('高频', 'High')}</span>
              </div>
            </div>

            <div className="overflow-x-auto pt-2">
              <div className="min-w-[640px] flex flex-col gap-1">
                {/* Hours Header */}
                <div className="flex items-center text-[10px] text-slate-400 font-mono pl-10">
                  {Array.from({ length: 24 }).map((_, h) => (
                    <div key={h} className="flex-1 text-center">
                      {h % 3 === 0 ? `${h}h` : ''}
                    </div>
                  ))}
                </div>

                {/* 7 Days Matrix */}
                {[1, 2, 3, 4, 5, 6, 0].map((dow) => {
                  return (
                    <div key={dow} className="flex items-center gap-1.5">
                      <span className="w-8 text-[11px] text-slate-500 dark:text-slate-400 font-medium shrink-0">
                        {dayNames[dow]}
                      </span>
                      <div className="flex-1 flex gap-1">
                        {Array.from({ length: 24 }).map((_, hr) => {
                          const point = data?.weekly_heatmap.find(
                            (p) => p.day_of_week === dow && p.hour === hr
                          );
                          const cnt = point ? point.count : 0;
                          return (
                            <div
                              key={hr}
                              title={ui(
                                `${dayNames[dow]} ${hr}:00~${hr}:59 累计访问: ${cnt} 次`,
                                `${dayNames[dow]} ${hr}:00–${hr}:59: ${cnt} visits`,
                              )}
                              className={`flex-1 h-5 rounded-sm transition-all cursor-pointer hover:ring-2 hover:ring-blue-500/50 ${getHeatmapColor(
                                cnt
                              )}`}
                            ></div>
                          );
                        })}
                      </div>
                    </div>
                  );
                })}
              </div>
            </div>
          </div>

          {/* 7. Search Queries & Forgotten Gems Grid */}
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            {/* Search Query Extraction TOP 20 */}
            <div className="bg-white dark:bg-slate-800 p-4 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-3">
              <div className="flex items-center justify-between font-medium text-slate-800 dark:text-slate-200">
                <div className="flex items-center gap-2">
                  <Search className="w-4 h-4 text-blue-600 dark:text-blue-400" />
                  <span>{ui('高频搜索词 Top 20', 'Top 20 search queries')}</span>
                </div>
                <span className="text-[10px] text-slate-400">{ui('点击在历史中检索', 'Click to filter history')}</span>
              </div>

              <div className="flex flex-col gap-1.5 max-h-72 overflow-y-auto pr-1">
                {!data?.top_search_queries || data.top_search_queries.length === 0 ? (
                  <span className="text-slate-400 py-6 text-center">{ui('暂未解析出搜索记录', 'No search queries extracted')}</span>
                ) : (
                  data.top_search_queries.map((sq, idx) => (
                    <button
                      key={`${sq.engine}-${sq.query}-${idx}`}
                      onClick={() => handleSearchClick(sq.query)}
                      className="flex items-center justify-between p-2 rounded hover:bg-slate-50 dark:hover:bg-slate-700/50 border border-slate-100 dark:border-slate-800 transition text-left group"
                    >
                      <div className="flex items-center gap-2 truncate pr-2">
                        <span className="text-[10px] font-mono text-slate-400 w-4 shrink-0">
                          {idx + 1}.
                        </span>
                        <span className="px-1.5 py-0.5 rounded text-[10px] bg-blue-50 text-blue-600 dark:bg-blue-950/60 dark:text-blue-300 shrink-0 font-medium">
                          {sq.engine}
                        </span>
                        <span className="font-medium text-slate-700 dark:text-slate-200 group-hover:text-blue-600 dark:group-hover:text-blue-400 truncate">
                          {sq.query}
                        </span>
                      </div>
                      <span className="font-mono text-slate-400 text-[11px] shrink-0">
                        {ui(`${sq.count} 次`, `${sq.count}x`)}
                      </span>
                    </button>
                  ))
                )}
              </div>
            </div>

            {/* Forgotten Gems */}
            <div className="bg-white dark:bg-slate-800 p-4 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-3">
              <div className="flex items-center justify-between font-medium text-slate-800 dark:text-slate-200">
                <div className="flex items-center gap-2">
                  <Sparkles className="w-4 h-4 text-amber-500" />
                  <span>{ui('长期未访的高频页面', 'Forgotten favorites')}</span>
                </div>
                <span className="text-[10px] text-slate-400">{ui('超过 30 天未再次访问', 'Not revisited in 30+ days')}</span>
              </div>

              <div className="flex flex-col gap-1.5 max-h-72 overflow-y-auto pr-1">
                {!data?.forgotten_gems || data.forgotten_gems.length === 0 ? (
                  <span className="text-slate-400 py-6 text-center">{ui('暂无符合条件的页面', 'No matching pages found')}</span>
                ) : (
                  data.forgotten_gems.map((gem) => (
                    <div
                      key={gem.url_id}
                      className="flex items-center justify-between p-2 rounded hover:bg-slate-50 dark:hover:bg-slate-700/50 border border-slate-100 dark:border-slate-800 transition text-left group"
                    >
                      <div className="flex flex-col gap-0.5 truncate pr-2">
                        <span className="font-medium text-slate-800 dark:text-slate-200 truncate">
                          {gem.title || gem.url}
                        </span>
                        <div className="flex items-center gap-2 text-[10px] text-slate-400 font-mono">
                          <span>{gem.domain}</span>
                          <span>·</span>
                          <span>{ui(`累计 ${gem.total_visits} 次`, `${gem.total_visits} visits`)}</span>
                          <span>·</span>
                          <span>{ui('上次访问 ', 'Last ')}{new Date(gem.last_visit_time).toLocaleDateString()}</span>
                        </div>
                      </div>
                      <button
                        onClick={() => tauriApi.openExternalUrl(gem.url)}
                        className="p-1.5 rounded hover:bg-blue-50 text-slate-400 hover:text-blue-600 dark:hover:bg-slate-600 transition shrink-0"
                        title={ui('在浏览器中重新打开', 'Open in browser')}
                      >
                        <ExternalLink className="w-3.5 h-3.5" />
                      </button>
                    </div>
                  ))
                )}
              </div>
            </div>
          </div>

          {/* 8. Browser Usage & 24h Distribution */}
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            {/* Browser Usage Ratio */}
            <div className="bg-white dark:bg-slate-800 p-4 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-3">
              <div className="flex items-center gap-2 font-medium text-slate-800 dark:text-slate-200">
                <Compass className="w-4 h-4 text-blue-600 dark:text-blue-400" />
                <span>{ui('浏览器占比分布', 'Browser share')}</span>
              </div>
              <div className="flex flex-col gap-3 mt-1">
                {!data?.browser_breakdown || data.browser_breakdown.length === 0 ? (
                  <span className="text-slate-400 py-4 text-center">{ui('暂无数据', 'No data')}</span>
                ) : (
                  data.browser_breakdown.map((b) => (
                    <div key={b.name} className="flex flex-col gap-1">
                      <div className="flex justify-between text-slate-700 dark:text-slate-300 capitalize">
                        <span className="font-medium">{b.name}</span>
                        <span className="font-mono">{b.count.toLocaleString()} ({b.percent}%)</span>
                      </div>
                      <div className="w-full h-2 bg-slate-100 dark:bg-slate-700 rounded-full overflow-hidden">
                        <div
                          className="h-full bg-blue-600 rounded-full transition-all"
                          style={{ width: `${b.percent}%` }}
                        ></div>
                      </div>
                    </div>
                  ))
                )}
              </div>
            </div>

            {/* 24-Hour Distribution */}
            <div className="bg-white dark:bg-slate-800 p-4 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-3">
              <div className="flex items-center gap-2 font-medium text-slate-800 dark:text-slate-200">
                <Clock className="w-4 h-4 text-amber-500" />
                <span>{ui('24 小时访问时段总热度分布', '24-hour activity distribution')}</span>
              </div>
              <div className="h-36 flex items-end gap-1.5 pt-4 pb-2 border-b border-slate-100 dark:border-slate-700">
                {data?.hourly_distribution.map((h) => {
                  const heightPercent = Math.max(Math.round((h.count / maxHourlyCount) * 100), 4);
                  return (
                    <div
                      key={h.hour}
                      className="flex-1 flex flex-col items-center gap-1 group relative h-full justify-end"
                    >
                      <div className="hidden group-hover:flex absolute bottom-full mb-1 bg-slate-900 text-white text-[10px] px-1.5 py-0.5 rounded whitespace-nowrap z-10 shadow">
                        {h.hour}:00 - {h.hour}:59 : {ui(`${h.count} 次`, `${h.count} visits`)}
                      </div>
                      <div
                        className="w-full bg-amber-500/80 hover:bg-amber-600 rounded-t transition-all cursor-pointer"
                        style={{ height: `${heightPercent}%` }}
                      ></div>
                      <span className="text-[9px] text-slate-400 font-mono scale-90">
                        {h.hour % 2 === 0 ? `${h.hour}h` : ''}
                      </span>
                    </div>
                  );
                })}
              </div>
            </div>
          </div>
        </div>
      ) : activeView === 'interests' ? (
        /* Interests & Evolution View */
        <div className="flex flex-col gap-6 mt-5">
          {/* AI Cognitive Evolution Report Header */}
          <div className="bg-blue-50/70 dark:bg-blue-950/20 p-4 rounded-xl border border-blue-200/70 dark:border-blue-800/40 shadow-sm flex flex-col gap-3">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div className="flex items-center gap-2.5">
                <div className="p-2 rounded-lg bg-blue-500/10 dark:bg-blue-400/10 text-blue-600 dark:text-blue-400">
                  <Brain className="w-5 h-5" />
                </div>
                <div>
                  <h3 className="font-semibold text-slate-800 dark:text-slate-100 text-sm flex items-center gap-2">
                    <span>{ui('兴趣演化对比', 'Interest evolution')}</span>
                    <span className="px-2 py-0.5 rounded-full text-[10px] bg-blue-100 dark:bg-blue-900/60 text-blue-700 dark:text-blue-300 font-medium">
                      {ui('环比分析', 'Period comparison')}
                    </span>
                  </h3>
                  <p className="text-slate-500 dark:text-slate-400 text-xs mt-0.5">
                    {ui('对比当前周期与上一周期的浏览主题变化，分析关注重心的迁移情况', 'Compare topic shifts between the current period and the previous period')}
                  </p>
                </div>
              </div>

              <button
                onClick={handleGeneratePeriodReport}
                disabled={isGeneratingReport}
                className="flex items-center gap-1.5 px-3 py-1.5 bg-blue-600 hover:bg-blue-700 disabled:opacity-50 text-white rounded-lg font-medium transition shadow-sm cursor-pointer"
              >
                {isGeneratingReport ? (
                  <Loader2 className="w-4 h-4 animate-spin" />
                ) : (
                  <Sparkles className="w-4 h-4" />
                )}
                <span>{isGeneratingReport ? ui('正在生成对比分析...', 'Generating comparison...') : ui('生成演化对比分析', 'Generate AI comparison')}</span>
              </button>
            </div>

            {aiReport && (
              <div className="mt-2 p-3.5 bg-white/90 dark:bg-slate-900/90 rounded-lg border border-slate-200/80 dark:border-slate-800 shadow-sm">
                <div className="flex items-center justify-between pb-2 border-b border-slate-100 dark:border-slate-800 mb-2">
                  <span className="font-medium text-slate-700 dark:text-slate-200 flex items-center gap-1.5 text-xs">
                    <FileText className="w-3.5 h-3.5 text-blue-600 dark:text-blue-400" />
                    <span>{ui('AI 报告正文', 'AI report')}</span>
                  </span>
                  <button
                    onClick={async () => {
                      try {
                        await navigator.clipboard.writeText(aiReport);
                        showToast(ui('报告已复制到剪贴板', 'Report copied to clipboard'), 'success');
                      } catch {
                        showToast(ui('复制到剪贴板失败', 'Failed to copy to clipboard'), 'error');
                      }
                    }}
                    className="text-slate-500 hover:text-slate-700 dark:hover:text-slate-300 text-[11px] underline cursor-pointer"
                  >
                    {ui('复制报告', 'Copy report')}
                  </button>
                </div>
                <div className="prose prose-xs dark:prose-invert max-w-none whitespace-pre-wrap text-slate-700 dark:text-slate-300 leading-relaxed font-sans text-xs">
                  {aiReport}
                </div>
              </div>
            )}
          </div>

          {isLoadingInterests ? (
            <div className="py-24 flex items-center justify-center text-slate-400 gap-2">
              <Loader2 className="w-4 h-4 animate-spin text-blue-600 dark:text-blue-400" />
              <span>{ui('正在加载分类体系与演化动态...', 'Loading topics and evolution...')}</span>
            </div>
          ) : (
            <>
              {interestsError && (
                <PanelError
                  message={interestsError}
                  staleRange={topics.length > 0 || evolution ? formatRange(interestsDataDays) : undefined}
                  onRetry={() => fetchInterests(days)}
                />
              )}
              {/* Evolution Dynamics (3 Columns: Rising, New, Declining) */}
              <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
                {/* Rising Topics */}
                <div className="bg-white dark:bg-slate-800 p-4 rounded-xl border border-emerald-100 dark:border-emerald-950/60 shadow-sm flex flex-col gap-3">
                  <div className="flex items-center justify-between pb-2 border-b border-slate-100 dark:border-slate-700/60">
                    <div className="flex items-center gap-2 font-semibold text-emerald-600 dark:text-emerald-400">
                      <TrendingUp className="w-4 h-4" />
                      <span>{ui('关注上升', 'Rising topics')}</span>
                    </div>
                    <span className="text-[11px] font-mono text-emerald-600/80 dark:text-emerald-400/80 bg-emerald-50 dark:bg-emerald-950/50 px-2 py-0.5 rounded-full">
                      {ui(`${evolution?.rising_topics.length || 0} 个主题`, `${evolution?.rising_topics.length || 0} topics`)}
                    </span>
                  </div>

                  <div className="flex flex-col gap-2">
                    {!evolution?.rising_topics.length ? (
                      <div className="py-8 text-center text-slate-400 text-xs">{ui('暂无激增主题', 'No surging topics')}</div>
                    ) : (
                      evolution.rising_topics.map((topic) => (
                        <div
                          key={topic.topic_id}
                          className="p-2.5 rounded-lg bg-emerald-50/50 dark:bg-emerald-950/20 border border-emerald-100/60 dark:border-emerald-900/40 flex items-center justify-between"
                        >
                          <div className="flex flex-col">
                            <span className="font-medium text-slate-800 dark:text-slate-100 text-xs">
                              {topic.name}
                            </span>
                            <span className="text-[10px] text-slate-400">
                              {ui(
                                `前周期 ${topic.previous_visits} 次 → 本期 ${topic.current_visits} 次`,
                                `Previous ${topic.previous_visits} → Current ${topic.current_visits}`,
                              )}
                            </span>
                          </div>
                          <span className="text-xs font-semibold text-emerald-600 dark:text-emerald-400 font-mono">
                            +{topic.growth_rate.toFixed(0)}%
                          </span>
                        </div>
                      ))
                    )}
                  </div>
                </div>

                {/* New Topics */}
                <div className="bg-white dark:bg-slate-800 p-4 rounded-xl border border-blue-100 dark:border-blue-950/60 shadow-sm flex flex-col gap-3">
                  <div className="flex items-center justify-between pb-2 border-b border-slate-100 dark:border-slate-700/60">
                    <div className="flex items-center gap-2 font-semibold text-blue-600 dark:text-blue-400">
                      <Sparkles className="w-4 h-4" />
                      <span>{ui('新增主题', 'New topics')}</span>
                    </div>
                    <span className="text-[11px] font-mono text-blue-600/80 dark:text-blue-400/80 bg-blue-50 dark:bg-blue-950/50 px-2 py-0.5 rounded-full">
                      {ui(`${evolution?.new_topics.length || 0} 个主题`, `${evolution?.new_topics.length || 0} topics`)}
                    </span>
                  </div>

                  <div className="flex flex-col gap-2">
                    {!evolution?.new_topics.length ? (
                      <div className="py-8 text-center text-slate-400 text-xs">{ui('本期暂无全新探索主题', 'No brand-new topics in this period')}</div>
                    ) : (
                      evolution.new_topics.map((topic) => (
                        <div
                          key={topic.topic_id}
                          className="p-2.5 rounded-lg bg-blue-50/50 dark:bg-blue-950/20 border border-blue-100/60 dark:border-blue-900/40 flex items-center justify-between"
                        >
                          <div className="flex flex-col">
                            <span className="font-medium text-slate-800 dark:text-slate-100 text-xs">
                              {topic.name}
                            </span>
                            <span className="text-[10px] text-slate-400">{ui('前周期未活跃', 'Inactive in previous period')}</span>
                          </div>
                          <span className="text-xs font-semibold text-blue-600 dark:text-blue-400 font-mono">
                            {ui(`${topic.current_visits} 次访问`, `${topic.current_visits} visits`)}
                          </span>
                        </div>
                      ))
                    )}
                  </div>
                </div>

                {/* Declining Topics */}
                <div className="bg-white dark:bg-slate-800 p-4 rounded-xl border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-3">
                  <div className="flex items-center justify-between pb-2 border-b border-slate-100 dark:border-slate-700/60">
                    <div className="flex items-center gap-2 font-semibold text-slate-500 dark:text-slate-400">
                      <TrendingDown className="w-4 h-4" />
                      <span>{ui('关注下降', 'Declining topics')}</span>
                    </div>
                    <span className="text-[11px] font-mono text-slate-500 bg-slate-100 dark:bg-slate-700 px-2 py-0.5 rounded-full">
                      {ui(`${evolution?.declining_topics.length || 0} 个主题`, `${evolution?.declining_topics.length || 0} topics`)}
                    </span>
                  </div>

                  <div className="flex flex-col gap-2">
                    {!evolution?.declining_topics.length ? (
                      <div className="py-8 text-center text-slate-400 text-xs">{ui('暂无明显降温主题', 'No cooling topics')}</div>
                    ) : (
                      evolution.declining_topics.map((topic) => (
                        <div
                          key={topic.topic_id}
                          className="p-2.5 rounded-lg bg-slate-50 dark:bg-slate-700/40 border border-slate-200 dark:border-slate-600/60 flex items-center justify-between"
                        >
                          <div className="flex flex-col">
                            <span className="font-medium text-slate-800 dark:text-slate-200 text-xs">
                              {topic.name}
                            </span>
                            <span className="text-[10px] text-slate-400">
                              {ui(
                                `前周期 ${topic.previous_visits} 次 → 本期 ${topic.current_visits} 次`,
                                `Previous ${topic.previous_visits} → Current ${topic.current_visits}`,
                              )}
                            </span>
                          </div>
                          <span className="text-xs font-semibold text-slate-500 dark:text-slate-400 font-mono">
                            {topic.growth_rate.toFixed(0)}%
                          </span>
                        </div>
                      ))
                    )}
                  </div>
                </div>
              </div>

              {/* Topics Taxonomy Grid */}
              <div className="bg-white dark:bg-slate-800 p-4 rounded-xl border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-3">
                <div className="flex items-center justify-between pb-2 border-b border-slate-100 dark:border-slate-700">
                  <div className="flex items-center gap-2">
                    <span className="font-semibold text-slate-800 dark:text-slate-100">
                      {ui(`主题分类统计 (${topics.length} 个类别)`, `Topic breakdown (${topics.length} categories)`)}
                    </span>
                  </div>
                  <span className="text-slate-400 text-[11px]">
                    {ui('根据规则与关键词对历史页面自动归类', 'Auto-classified via domain and keyword rules')}
                  </span>
                </div>

                <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
                  {topics.map((top) => (
                    <div
                      key={top.id}
                      className="p-3 rounded-lg border border-slate-200 dark:border-slate-700 bg-slate-50/60 dark:bg-slate-900/40 flex flex-col gap-1.5"
                    >
                      <div className="flex items-center justify-between">
                        <span className="font-medium text-slate-700 dark:text-slate-200 text-xs">
                          {top.name}
                        </span>
                        <span
                          className="w-2.5 h-2.5 rounded-full"
                          style={{ backgroundColor: top.color || '#3b82f6' }}
                        />
                      </div>
                      <div className="flex items-center justify-between text-[11px] text-slate-500 dark:text-slate-400 pt-1 border-t border-slate-200/60 dark:border-slate-800">
                        <span>{ui(`累计访问: ${top.visit_count} 次`, `Visits: ${top.visit_count}`)}</span>
                        <span>{ui(`${top.url_count} 网页`, `${top.url_count} pages`)}</span>
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            </>
          )}
        </div>
      ) : activeView === 'sessions' ? (
        /* Sessions View */
        <div className="flex flex-col gap-3 mt-5">
          <div className="flex items-center justify-between pb-2 border-b border-slate-200 dark:border-slate-800">
            <span className="text-slate-500 text-xs">
              {ui(
                `按 30 分钟无操作切分的连续浏览会话 (共 ${sessions.length} 个会话)`,
                `Browsing sessions split at 30-minute idle gaps (${sessions.length} sessions)`,
              )}
            </span>
          </div>

          {isLoadingSessions ? (
            <div className="py-24 flex items-center justify-center text-slate-400 gap-2">
              <Loader2 className="w-4 h-4 animate-spin text-blue-600 dark:text-blue-400" />
              <span>{ui('正在聚类研究会话...', 'Clustering research sessions...')}</span>
            </div>
          ) : sessions.length === 0 ? (
            sessionsError ? (
              <PanelError message={sessionsError} onRetry={() => fetchSessions(days)} />
            ) : (
              <div className="py-20 text-center text-slate-400 text-xs">{ui('当前时段暂无聚类会话', 'No sessions found in selected range')}</div>
            )
          ) : (
            <>
              {sessionsError && (
                <PanelError
                  message={sessionsError}
                  staleRange={formatRange(sessionsDataDays)}
                  onRetry={() => fetchSessions(days)}
                />
              )}
              <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                {sessions.map((ses) => (
                <div
                  key={ses.session_id}
                  className="bg-white dark:bg-slate-800 p-3.5 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-2 hover:border-blue-300 dark:hover:border-blue-700 transition"
                >
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2 font-mono text-[11px] text-slate-400">
                      <Clock className="w-3 h-3 text-blue-600 dark:text-blue-400" />
                      <span>{new Date(ses.start_time).toLocaleString()}</span>
                    </div>
                    <span className="px-2 py-0.5 rounded-full text-[10px] bg-slate-100 dark:bg-slate-700 text-slate-600 dark:text-slate-300 font-medium">
                      {ui('时长 ', 'Duration ')}{formatDurationCompact(ses.duration_secs, language)}
                    </span>
                  </div>

                  <div className="font-semibold text-slate-800 dark:text-slate-100 text-xs line-clamp-1">
                    {ses.sample_title || ses.dominant_domain}
                  </div>

                  <div className="flex items-center justify-between pt-1 border-t border-slate-100 dark:border-slate-700 text-[11px] text-slate-500">
                    <div className="flex items-center gap-1.5">
                      <Globe className="w-3 h-3 text-slate-400" />
                      <span className="font-mono text-slate-700 dark:text-slate-300 font-medium">
                        {ses.dominant_domain || ui('未知站点', 'Unknown site')}
                      </span>
                    </div>
                    <div className="flex items-center gap-2">
                      <span className="font-medium text-blue-600 dark:text-blue-400">
                        {ui(`${ses.visit_count} 个页面`, `${ses.visit_count} pages`)}
                      </span>
                      <span>·</span>
                      <span>{ses.browser}</span>
                    </div>
                  </div>
                </div>
                ))}
              </div>
            </>
          )}
        </div>
      ) : (
        /* On This Day View */
        <div className="flex flex-col gap-4 mt-5">
          {/* Subheader & Date Picker */}
          <div className="bg-white dark:bg-slate-800 p-4 rounded-xl border border-slate-200 dark:border-slate-700 shadow-sm flex flex-wrap items-center justify-between gap-3">
            <div className="flex items-center gap-3">
              <div className="p-2 rounded-lg bg-blue-50 dark:bg-blue-950/60 text-blue-600 dark:text-blue-400">
                <Calendar className="w-5 h-5" />
              </div>
              <div>
                <h3 className="font-semibold text-slate-800 dark:text-slate-100 text-sm flex items-center gap-2">
                  <span>{ui('历史上的今天', 'On this day')}</span>
                  <span className="px-2 py-0.5 rounded-full text-[10px] bg-blue-100 dark:bg-blue-900/60 text-blue-700 dark:text-blue-300 font-medium font-mono">
                    {ui(`${onThisDayResult?.total_count ?? 0} 条记录`, `${onThisDayResult?.total_count ?? 0} records`)}
                  </span>
                </h3>
                <p className="text-slate-500 dark:text-slate-400 text-xs mt-0.5">
                  {ui('查看往年在这一天浏览过的内容', 'Revisit pages browsed on this calendar day in past years')}
                </p>
              </div>
            </div>

            <div className="flex items-center gap-2">
              <span className="text-slate-500 text-xs">{ui('选择日期:', 'Date:')}</span>
              <input
                type="date"
                value={onThisDayDate}
                onChange={(e) => {
                  const val = e.target.value;
                  setOnThisDayDate(val);
                  setCurrentPage(1);
                  fetchOnThisDayData(val, selectedYear, 1);
                }}
                aria-label={ui('选择日期', 'Select date')}
                className="px-2.5 py-1.5 rounded-md border border-slate-200 dark:border-slate-700 bg-slate-50 dark:bg-slate-900 text-slate-800 dark:text-slate-200 text-xs font-mono font-medium focus:outline-none focus:border-blue-500 cursor-pointer"
              />
              <button
                type="button"
                onClick={() => {
                  const today = new Date();
                  const y = today.getFullYear();
                  const m = String(today.getMonth() + 1).padStart(2, '0');
                  const d = String(today.getDate()).padStart(2, '0');
                  const todayStr = `${y}-${m}-${d}`;
                  setOnThisDayDate(todayStr);
                  setCurrentPage(1);
                  fetchOnThisDayData(todayStr, selectedYear, 1);
                }}
                className="px-2.5 py-1.5 rounded-md border border-slate-200 dark:border-slate-700 hover:bg-slate-100 dark:hover:bg-slate-700 text-slate-600 dark:text-slate-300 text-xs transition cursor-pointer"
              >
                {ui('回到今日', 'Today')}
              </button>
            </div>
          </div>

          {/* Years Filter Pills */}
          {onThisDayResult && onThisDayResult.available_years && onThisDayResult.available_years.length > 0 && (
            <div className="flex items-center gap-2 flex-wrap bg-white dark:bg-slate-800 p-3 rounded-xl border border-slate-200 dark:border-slate-700 shadow-xs text-xs">
              <span className="text-slate-400 text-xs font-medium">{ui('历年筛选:', 'Years:')}</span>
              <button
                type="button"
                onClick={() => {
                  setSelectedYear(undefined);
                  setCurrentPage(1);
                  fetchOnThisDayData(onThisDayDate, undefined, 1);
                }}
                className={`px-3 py-1 rounded-lg text-xs font-medium transition cursor-pointer ${
                  selectedYear === undefined
                    ? 'bg-blue-600 text-white shadow-xs'
                    : 'bg-slate-100 dark:bg-slate-700/80 text-slate-600 dark:text-slate-300 hover:bg-slate-200 dark:hover:bg-slate-700'
                }`}
              >
                {ui(`全部历年 (${onThisDayResult.total_count})`, `All years (${onThisDayResult.total_count})`)}
              </button>
              {onThisDayResult.available_years.map((yr) => (
                <button
                  key={yr}
                  type="button"
                  onClick={() => {
                    setSelectedYear(yr);
                    setCurrentPage(1);
                    fetchOnThisDayData(onThisDayDate, yr, 1);
                  }}
                  className={`px-3 py-1 rounded-lg text-xs font-medium transition cursor-pointer ${
                    selectedYear === yr
                      ? 'bg-blue-600 text-white shadow-xs'
                      : 'bg-slate-100 dark:bg-slate-700/80 text-slate-600 dark:text-slate-300 hover:bg-slate-200 dark:hover:bg-slate-700'
                  }`}
                >
                  {ui(`${yr} 年`, `${yr}`)}
                </button>
              ))}
            </div>
          )}

          {/* List of On This Day items */}
          {isLoadingOnThisDay ? (
            <div className="py-24 flex items-center justify-center text-slate-400 gap-2">
              <Loader2 className="w-4 h-4 animate-spin text-blue-600 dark:text-blue-400" />
              <span>{ui('正在加载历史记录...', 'Loading history items...')}</span>
            </div>
          ) : onThisDayError && !onThisDayResult ? (
            <PanelError message={onThisDayError} onRetry={() => fetchOnThisDayData(onThisDayDate, selectedYear, currentPage)} />
          ) : !onThisDayResult || onThisDayResult.items.length === 0 ? (
            <div className="bg-white dark:bg-slate-800 p-12 rounded-xl border border-slate-200 dark:border-slate-700 text-center flex flex-col items-center justify-center gap-2">
              {onThisDayError && (
                <PanelError
                  message={onThisDayError}
                  staleRange={onThisDayDataKey ? onThisDayDataKey.split(':').slice(0, 2).join(' / ') : undefined}
                  onRetry={() => fetchOnThisDayData(onThisDayDate, selectedYear, currentPage)}
                />
              )}
              <History className="w-8 h-8 text-slate-300 dark:text-slate-600" />
              <p className="text-slate-500 dark:text-slate-400 text-xs">
                {ui('在所选日期的往年中暂无浏览记录', 'No browsing records found for this date in past years')}
              </p>
              <span className="text-slate-400 text-[11px]">
                {ui('尝试切换其他日期或导入更早的浏览器历史归档', 'Try another date or import older browser archives')}
              </span>
            </div>
          ) : (
            <>
              {onThisDayError && (
                <PanelError
                  message={onThisDayError}
                  staleRange={onThisDayDataKey ? onThisDayDataKey.split(':').slice(0, 2).join(' / ') : undefined}
                  onRetry={() => fetchOnThisDayData(onThisDayDate, selectedYear, currentPage)}
                />
              )}
              <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                {onThisDayResult.items.map((item) => (
                  <div
                    key={item.id}
                    className="bg-white dark:bg-slate-800 p-3.5 rounded-xl border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-2 hover:border-blue-300 dark:hover:border-blue-700 transition group"
                  >
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-1.5">
                        <span className="px-2 py-0.5 rounded-full text-[10px] font-semibold bg-amber-50 dark:bg-amber-950/60 text-amber-600 dark:text-amber-400 border border-amber-200/60 dark:border-amber-800/40">
                          {ui(`${item.years_ago} 年前的今天`, `${item.years_ago}y ago`)}
                        </span>
                        <span className="text-[11px] font-mono text-slate-400">
                          {new Date(item.visit_time).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
                        </span>
                      </div>
                      <div className="flex items-center gap-1 opacity-80 group-hover:opacity-100 transition">
                        <button
                          type="button"
                          onClick={() => handleSearchClick(item.domain)}
                          className="p-1 rounded text-slate-400 hover:text-blue-600 hover:bg-slate-100 dark:hover:bg-slate-700 transition cursor-pointer"
                          title={ui('在历史记录中检索该域名', 'Filter history by domain')}
                        >
                          <Search className="w-3.5 h-3.5" />
                        </button>
                        <button
                          type="button"
                          onClick={() => tauriApi.openExternalUrl(item.url)}
                          className="p-1 rounded text-slate-400 hover:text-blue-600 hover:bg-slate-100 dark:hover:bg-slate-700 transition cursor-pointer"
                          title={ui('在默认浏览器中打开', 'Open in browser')}
                        >
                          <ExternalLink className="w-3.5 h-3.5" />
                        </button>
                      </div>
                    </div>

                    <a
                      href={item.url}
                      onClick={(e) => {
                        e.preventDefault();
                        tauriApi.openExternalUrl(item.url);
                      }}
                      className="font-semibold text-slate-800 dark:text-slate-100 text-xs line-clamp-2 hover:text-blue-600 dark:hover:text-blue-400 transition-colors"
                    >
                      {item.title || item.url}
                    </a>

                    <div className="flex items-center justify-between pt-1.5 border-t border-slate-100 dark:border-slate-700/80 text-[11px] text-slate-500">
                      <div className="flex items-center gap-1.5 truncate pr-2">
                        <Globe className="w-3 h-3 text-slate-400 shrink-0" />
                        <span className="font-mono text-slate-600 dark:text-slate-300 font-medium truncate">
                          {item.domain || ui('未知域名', 'Unknown domain')}
                        </span>
                      </div>
                      <span className="font-mono text-slate-400 text-[10px] shrink-0">
                        {new Date(item.visit_time).toLocaleDateString()}
                      </span>
                    </div>
                  </div>
                ))}
              </div>

              {/* Pagination Controls */}
              {onThisDayResult.total_count > 0 && (
                <div className="flex items-center justify-between bg-white dark:bg-slate-800 p-3 rounded-xl border border-slate-200 dark:border-slate-700 shadow-xs text-xs mt-2">
                  <span className="text-slate-500 font-mono text-[11px]">
                    {ui(`共 ${onThisDayResult.total_count} 条记录 · 每页 30 条`, `${onThisDayResult.total_count} records · 30 per page`)}
                  </span>
                  <div className="flex items-center gap-2">
                    <button
                      type="button"
                      disabled={currentPage <= 1 || isLoadingOnThisDay}
                      onClick={() => {
                        const prev = Math.max(1, currentPage - 1);
                        setCurrentPage(prev);
                        fetchOnThisDayData(onThisDayDate, selectedYear, prev);
                      }}
                      className="px-3 py-1 rounded border border-slate-200 dark:border-slate-700 bg-slate-50 dark:bg-slate-900 hover:bg-slate-100 dark:hover:bg-slate-800 disabled:opacity-40 disabled:cursor-not-allowed transition text-slate-700 dark:text-slate-200 cursor-pointer"
                    >
                      {ui('上一页', 'Previous')}
                    </button>
                    <span className="text-slate-600 dark:text-slate-400 font-medium font-mono px-1">
                      {ui(`第 ${onThisDayResult.page} / ${onThisDayResult.total_pages || 1} 页`, `Page ${onThisDayResult.page} / ${onThisDayResult.total_pages || 1}`)}
                    </span>
                    <button
                      type="button"
                      disabled={currentPage >= onThisDayResult.total_pages || isLoadingOnThisDay}
                      onClick={() => {
                        const next = currentPage + 1;
                        setCurrentPage(next);
                        fetchOnThisDayData(onThisDayDate, selectedYear, next);
                      }}
                      className="px-3 py-1 rounded border border-slate-200 dark:border-slate-700 bg-slate-50 dark:bg-slate-900 hover:bg-slate-100 dark:hover:bg-slate-800 disabled:opacity-40 disabled:cursor-not-allowed transition text-slate-700 dark:text-slate-200 cursor-pointer"
                    >
                      {ui('下一页', 'Next')}
                    </button>
                  </div>
                </div>
              )}
            </>
          )}
        </div>
      )}

      {/* Domain Detail Modal */}
      {selectedDomain && (
        <DomainDetailModal
          domain={selectedDomain}
          startTime={startTime}
          endTime={endTime}
          onClose={() => setSelectedDomain(null)}
        />
      )}
    </div>
  );
};
