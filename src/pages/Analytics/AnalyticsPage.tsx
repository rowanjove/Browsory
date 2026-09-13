import React, { useEffect, useState } from 'react';
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

export const AnalyticsPage: React.FC = () => {
  const { showToast, setCurrentTab } = useAppStore();
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

  const startTime = days > 0 ? Date.now() - days * 86400 * 1000 : undefined;
  const endTime = undefined;

  const fetchAnalytics = async (selectedDays: number) => {
    setIsLoading(true);
    const start = selectedDays > 0 ? Date.now() - selectedDays * 86400 * 1000 : undefined;
    try {
      const [summary, dyn] = await Promise.all([
        tauriApi.getAnalytics(selectedDays === 0 ? undefined : selectedDays),
        tauriApi.getDomainDynamics(start),
      ]);
      setData(summary);
      setDynamics(dyn);
    } catch (err: any) {
      showToast(err?.message || '获取分析统计失败', 'error');
    } finally {
      setIsLoading(false);
    }
  };

  const fetchRanking = async (selectedDays: number, limit: number) => {
    setIsLoadingRanking(true);
    const start = selectedDays > 0 ? Date.now() - selectedDays * 86400 * 1000 : undefined;
    try {
      const list = await tauriApi.getWebsiteRanking(start, undefined, limit);
      setRanking(list);
    } catch (err: any) {
      console.error('获取网站排名失败:', err);
    } finally {
      setIsLoadingRanking(false);
    }
  };

  const fetchInterests = async (selectedDays: number) => {
    setIsLoadingInterests(true);
    try {
      const [tList, evo] = await Promise.all([
        tauriApi.getAllTopics(),
        tauriApi.getInterestEvolution(selectedDays === 0 ? 30 : selectedDays),
      ]);
      setTopics(tList);
      setEvolution(evo);
    } catch (err: any) {
      console.error('获取兴趣演化失败:', err);
    } finally {
      setIsLoadingInterests(false);
    }
  };

  const handleGeneratePeriodReport = async () => {
    setIsGeneratingReport(true);
    try {
      const report = await tauriApi.generateAiPeriodComparison(days === 0 ? 30 : days);
      setAiReport(report);
      showToast('AI 认知演化报告生成完毕', 'success');
    } catch (err: any) {
      showToast(err?.message || '生成 AI 报告失败，请检查设置中的 AI 配置', 'error');
    } finally {
      setIsGeneratingReport(false);
    }
  };

  const fetchSessions = async (selectedDays: number) => {
    setIsLoadingSessions(true);
    try {
      const list = await tauriApi.getResearchSessions(selectedDays === 0 ? undefined : selectedDays, 50);
      setSessions(list);
    } catch (err: any) {
      showToast(err?.message || '获取研究会话失败', 'error');
    } finally {
      setIsLoadingSessions(false);
    }
  };

  const fetchOnThisDayData = async (targetDate?: string, year?: number, page: number = 1) => {
    setIsLoadingOnThisDay(true);
    try {
      const result = await tauriApi.getOnThisDay(targetDate, year, page, 30);
      setOnThisDayResult(result);
    } catch (err: any) {
      console.error('获取历史上的今天失败:', err);
    } finally {
      setIsLoadingOnThisDay(false);
    }
  };

  useEffect(() => {
    fetchAnalytics(days);
    fetchRanking(days, rankingLimit);
    if (activeView === 'interests') {
      fetchInterests(days);
    } else if (activeView === 'sessions') {
      fetchSessions(days);
    } else if (activeView === 'on_this_day') {
      fetchOnThisDayData(onThisDayDate, selectedYear, currentPage);
    }
  }, [days, activeView]);

  useEffect(() => {
    fetchRanking(days, rankingLimit);
  }, [rankingLimit]);

  const handleSearchClick = (keyword: string) => {
    setSearch(keyword);
    setCurrentTab('history');
  };

  // Calculations for charts
  const maxDailyCount = Math.max(...(data?.daily_trend.map((d) => d.count) || [1]), 1);
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

  const dayNames = ['周日', '周一', '周二', '周三', '周四', '周五', '周六'];

  const formatDuration = (secs: number) => {
    if (secs < 60) return `${secs}秒`;
    const mins = Math.floor(secs / 60);
    if (mins < 60) return `${mins}分钟`;
    const hrs = Math.floor(mins / 60);
    const remMins = mins % 60;
    return `${hrs}小时${remMins > 0 ? `${remMins}分` : ''}`;
  };

  return (
    <div className="flex-1 flex flex-col h-full bg-slate-50 dark:bg-slate-900 overflow-y-auto p-6 text-xs select-none">
      {/* Header with time scope selector & View Tabs */}
      <div className="flex items-center justify-between pb-4 border-b border-slate-200 dark:border-slate-800">
        <div className="flex flex-col gap-1">
          <div className="flex items-center gap-3">
            <h1 className="text-base font-semibold text-slate-800 dark:text-slate-100 flex items-center gap-2">
              <TrendingUp className="w-4 h-4 text-blue-500" />
              <span>统计分析</span>
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
                <span>全维洞察</span>
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
                <span>知识图谱与演化</span>
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
                <span>研究会话流</span>
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
                <span>历史上的今天</span>
              </button>
            </div>
          </div>
          <p className="text-slate-500">
            Local-First 本地隐私安全聚合 · 当前区间归档 {data?.total_visits.toLocaleString() ?? '...'} 次访问
          </p>
        </div>

        <div className="flex items-center gap-2">
          <div className="flex items-center gap-1 border border-slate-300 dark:border-slate-700 rounded px-2 py-1 bg-white dark:bg-slate-800">
            <Calendar className="w-3.5 h-3.5 text-slate-400" />
            <select
              value={days}
              onChange={(e) => setDays(Number(e.target.value))}
              className="bg-transparent text-slate-700 dark:text-slate-200 focus:outline-none cursor-pointer text-xs"
            >
              <option value={7}>最近 7 天</option>
              <option value={30}>最近 30 天</option>
              <option value={90}>最近 90 天</option>
              <option value={365}>今年 (最近 1 年)</option>
              <option value={0}>全量历史记录</option>
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
            title="刷新统计"
          >
            <RefreshCw className={`w-3.5 h-3.5 ${isLoading || isLoadingSessions ? 'animate-spin' : ''}`} />
          </button>
        </div>
      </div>

      {isLoading && !data ? (
        <div className="flex-1 flex items-center justify-center py-24 text-slate-400 gap-2">
          <Loader2 className="w-4 h-4 animate-spin text-blue-500" />
          <span>正在聚合全维历史数据...</span>
        </div>
      ) : activeView === 'overview' ? (
        <div className="flex flex-col gap-5 mt-5">
          {/* 1. Extended KPI Cards (5 Metrics) */}
          <div className="grid grid-cols-2 md:grid-cols-5 gap-3">
            {/* Total Visits with Period Growth */}
            <div className="bg-white dark:bg-slate-800 p-3.5 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-1.5">
              <div className="flex items-center justify-between text-slate-400">
                <span className="text-[11px] font-medium">总访问量</span>
                <Activity className="w-3.5 h-3.5 text-blue-500" />
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
                    <span className="text-slate-400">环比上一周期</span>
                  </>
                ) : (
                  <span className="text-slate-400">全历史累计</span>
                )}
              </div>
            </div>

            {/* Unique URLs */}
            <div className="bg-white dark:bg-slate-800 p-3.5 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-1.5">
              <div className="flex items-center justify-between text-slate-400">
                <span className="text-[11px] font-medium">独立网页数</span>
                <FileText className="w-3.5 h-3.5 text-indigo-500" />
              </div>
              <div className="text-xl font-bold font-mono text-slate-800 dark:text-slate-100">
                {data?.unique_urls.toLocaleString() ?? 0}
              </div>
              <div className="text-[10px] text-slate-400">
                均访 {data && data.unique_urls > 0 ? (data.total_visits / data.unique_urls).toFixed(1) : 1} 次/页
              </div>
            </div>

            {/* Unique Domains */}
            <div className="bg-white dark:bg-slate-800 p-3.5 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-1.5">
              <div className="flex items-center justify-between text-slate-400">
                <span className="text-[11px] font-medium">独立域名数</span>
                <Globe className="w-3.5 h-3.5 text-cyan-500" />
              </div>
              <div className="text-xl font-bold font-mono text-slate-800 dark:text-slate-100">
                {data?.unique_domains.toLocaleString() ?? 0}
              </div>
              <div className="text-[10px] text-slate-400">
                涉及站点总广度
              </div>
            </div>

            {/* Active Days */}
            <div className="bg-white dark:bg-slate-800 p-3.5 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-1.5">
              <div className="flex items-center justify-between text-slate-400">
                <span className="text-[11px] font-medium">活跃天数</span>
                <Calendar className="w-3.5 h-3.5 text-amber-500" />
              </div>
              <div className="text-xl font-bold font-mono text-slate-800 dark:text-slate-100">
                {data?.active_days ?? 0} <span className="text-xs font-normal text-slate-400">天</span>
              </div>
              <div className="text-[10px] text-slate-400">
                {days > 0 ? `活跃率 ${Math.min(Math.round(((data?.active_days || 0) / days) * 100), 100)}%` : '持续活跃'}
              </div>
            </div>

            {/* Revisit Rate */}
            <div className="bg-white dark:bg-slate-800 p-3.5 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-1.5">
              <div className="flex items-center justify-between text-slate-400">
                <span className="text-[11px] font-medium">网页重访率</span>
                <Repeat className="w-3.5 h-3.5 text-emerald-500" />
              </div>
              <div className="text-xl font-bold font-mono text-emerald-600 dark:text-emerald-400">
                {data?.revisit_rate ?? 0}%
              </div>
              <div className="text-[10px] text-slate-400">
                反复查阅的沉淀知识
              </div>
            </div>
          </div>

          {/* 2. Deep Website Analysis & Ranking (网站深度排行 Top 20/50/100) */}
          <div className="bg-white dark:bg-slate-800 p-4 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-3">
            <div className="flex items-center justify-between pb-2 border-b border-slate-100 dark:border-slate-700">
              <div className="flex items-center gap-2">
                <Globe className="w-4 h-4 text-blue-500" />
                <span className="font-semibold text-slate-800 dark:text-slate-100">
                  网站深度排行榜 (Website Ranking)
                </span>
                <span className="text-slate-400 text-[11px]">
                  · 支持点击任意站点下钻路径树 (URL Path Tree) 与小时分布
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
                <Loader2 className="w-4 h-4 animate-spin text-blue-500" />
                <span>正在计算站点深度排行榜...</span>
              </div>
            ) : ranking.length === 0 ? (
              <div className="py-8 text-center text-slate-400">所选时段暂无网站数据</div>
            ) : (
              <div className="overflow-x-auto">
                <table className="w-full text-left border-collapse">
                  <thead>
                    <tr className="border-b border-slate-100 dark:border-slate-700/60 text-slate-400 text-[11px]">
                      <th className="pb-2 font-medium w-12 text-center">排名</th>
                      <th className="pb-2 font-medium">域名 / 站点</th>
                      <th className="pb-2 font-medium text-right">访问次数</th>
                      <th className="pb-2 font-medium text-right">涉及页面数</th>
                      <th className="pb-2 font-medium text-right">重访率</th>
                      <th className="pb-2 font-medium text-right">活跃天数</th>
                      <th className="pb-2 font-medium text-right">最近访问</th>
                      <th className="pb-2 font-medium text-center w-24">操作</th>
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
                            {item.active_days} 天
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
                                下钻深度
                              </button>
                              <button
                                onClick={() => handleSearchClick(item.domain)}
                                className="p-1 rounded text-slate-400 hover:text-blue-600 hover:bg-slate-100 dark:hover:bg-slate-700 transition"
                                title="在历史记录中检索"
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
            )}
          </div>

          {/* 4. Domain Dynamics Discovery (新发现 vs 沉寂网站) */}
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            {/* New Domains */}
            <div className="bg-white dark:bg-slate-800 p-4 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-3">
              <div className="flex items-center justify-between pb-2 border-b border-slate-100 dark:border-slate-700">
                <div className="flex items-center gap-2 font-semibold text-slate-800 dark:text-slate-100">
                  <Sparkles className="w-4 h-4 text-emerald-500" />
                  <span>新探索发现网站 (New Domains)</span>
                </div>
                <span className="text-[11px] text-emerald-600 dark:text-emerald-400 font-mono">
                  {dynamics?.new_domains.length ?? 0} 个站点
                </span>
              </div>

              <div className="flex flex-col gap-1.5 max-h-72 overflow-y-auto pr-1">
                {!dynamics?.new_domains || dynamics.new_domains.length === 0 ? (
                  <span className="text-slate-400 py-6 text-center">本时段暂无全新探索站点</span>
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
                            初次探索
                          </span>
                        </div>
                        <span className="text-[11px] text-slate-500 dark:text-slate-400 truncate">
                          {nd.sample_title || nd.sample_url}
                        </span>
                      </div>
                      <div className="flex flex-col items-end gap-0.5 shrink-0">
                        <span className="font-mono text-emerald-600 dark:text-emerald-400 font-bold">
                          {nd.visits_in_period} 次
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
                  <span>历史高频 · 近期沉寂网站 (Dormant)</span>
                </div>
                <span className="text-[11px] text-slate-400 font-mono">
                  {dynamics?.dormant_domains.length ?? 0} 个站点
                </span>
              </div>

              <div className="flex flex-col gap-1.5 max-h-72 overflow-y-auto pr-1">
                {!dynamics?.dormant_domains || dynamics.dormant_domains.length === 0 ? (
                  <span className="text-slate-400 py-6 text-center">暂无显著沉寂的高频站点</span>
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
                          历史累计 {dd.total_historical_visits} 次访问 · 上次访问于 {new Date(dd.last_visit_time).toLocaleDateString()}
                        </span>
                      </div>
                      <div className="flex items-center gap-2 shrink-0">
                        <span className="px-2 py-0.5 rounded bg-slate-100 dark:bg-slate-700 text-slate-500 dark:text-slate-400 text-[10px] font-mono">
                          沉寂 {dd.days_dormant} 天
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
          <div className="bg-white dark:bg-slate-800 p-4 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-3">
            <div className="flex items-center justify-between font-medium text-slate-800 dark:text-slate-200">
              <div className="flex items-center gap-2">
                <TrendingUp className="w-4 h-4 text-emerald-500" />
                <span>每日访问量趋势 (Daily Trend)</span>
              </div>
              <span className="text-slate-400 text-[11px] font-mono">
                {data?.daily_trend.length ?? 0} 天记录
              </span>
            </div>

            {!data?.daily_trend || data.daily_trend.length === 0 ? (
              <span className="text-slate-400 py-8 text-center">所选时段暂无访问记录</span>
            ) : (
              <div className="flex flex-col">
                <div className="h-32 flex items-end gap-1 px-1 relative">
                  {data.daily_trend.map((d) => {
                    const heightPercent = Math.max(Math.round((d.count / maxDailyCount) * 100), 4);
                    return (
                      <div
                        key={d.date}
                        className="flex-1 min-w-[14px] max-w-[32px] h-full flex items-end justify-center group relative"
                      >
                        {/* Tooltip */}
                        <div className="hidden group-hover:flex absolute bottom-[calc(100%+4px)] left-1/2 -translate-x-1/2 pointer-events-none z-30 bg-slate-900/95 text-white text-[10px] px-2 py-0.5 rounded shadow-lg whitespace-nowrap font-mono">
                          {d.date}: {d.count} 次
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
                <div className="h-5 pt-1 border-t border-slate-200 dark:border-slate-700/80 flex items-center gap-1 px-1">
                  {data.daily_trend.map((d, i) => {
                    const step = Math.max(Math.floor((data.daily_trend.length || 1) / 8), 1);
                    const showLabel = i % step === 0 || i === data.daily_trend.length - 1;
                    return (
                      <div
                        key={d.date}
                        className="flex-1 min-w-[14px] max-w-[32px] text-center overflow-visible"
                      >
                        {showLabel && (
                          <span className="text-[9px] text-slate-400 dark:text-slate-500 font-mono block whitespace-nowrap -ml-2">
                            {d.date.slice(5)}
                          </span>
                        )}
                      </div>
                    );
                  })}
                </div>
              </div>
            )}
          </div>

          {/* 6. 7x24 Weekly-Hourly Heatmap */}
          <div className="bg-white dark:bg-slate-800 p-4 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-3">
            <div className="flex items-center justify-between font-medium text-slate-800 dark:text-slate-200">
              <div className="flex items-center gap-2">
                <Flame className="w-4 h-4 text-orange-500" />
                <span>7×24 小时生活节奏热力图 (Weekly-Hourly Heatmap)</span>
              </div>
              <div className="flex items-center gap-1 text-[11px] text-slate-400">
                <span>低</span>
                <span className="w-2.5 h-2.5 rounded-sm bg-slate-100 dark:bg-slate-800 border border-slate-300 dark:border-slate-700"></span>
                <span className="w-2.5 h-2.5 rounded-sm bg-emerald-200 dark:bg-emerald-950"></span>
                <span className="w-2.5 h-2.5 rounded-sm bg-emerald-400 dark:bg-emerald-800"></span>
                <span className="w-2.5 h-2.5 rounded-sm bg-emerald-600 dark:bg-emerald-500"></span>
                <span>高频</span>
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
                              title={`${dayNames[dow]} ${hr}:00~${hr}:59 累计访问: ${cnt} 次`}
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
                  <Search className="w-4 h-4 text-blue-500" />
                  <span>高频搜索关键词 (Search Query TOP 20)</span>
                </div>
                <span className="text-[10px] text-slate-400">点击在历史中检索</span>
              </div>

              <div className="flex flex-col gap-1.5 max-h-72 overflow-y-auto pr-1">
                {!data?.top_search_queries || data.top_search_queries.length === 0 ? (
                  <span className="text-slate-400 py-6 text-center">暂未解析出搜索记录</span>
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
                        {sq.count} 次
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
                  <span>遗忘宝藏发现 (曾经高频 · 逾 30 天未访)</span>
                </div>
                <span className="text-[10px] text-slate-400">重温深度价值</span>
              </div>

              <div className="flex flex-col gap-1.5 max-h-72 overflow-y-auto pr-1">
                {!data?.forgotten_gems || data.forgotten_gems.length === 0 ? (
                  <span className="text-slate-400 py-6 text-center">暂无遗忘宝藏</span>
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
                          <span>累计 {gem.total_visits} 次</span>
                          <span>·</span>
                          <span>上次访问 {new Date(gem.last_visit_time).toLocaleDateString()}</span>
                        </div>
                      </div>
                      <button
                        onClick={() => tauriApi.openExternalUrl(gem.url)}
                        className="p-1.5 rounded hover:bg-blue-50 text-slate-400 hover:text-blue-600 dark:hover:bg-slate-600 transition shrink-0"
                        title="在浏览器中重新打开"
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
                <Compass className="w-4 h-4 text-indigo-500" />
                <span>浏览器占比分布</span>
              </div>
              <div className="flex flex-col gap-3 mt-1">
                {!data?.browser_breakdown || data.browser_breakdown.length === 0 ? (
                  <span className="text-slate-400 py-4 text-center">暂无数据</span>
                ) : (
                  data.browser_breakdown.map((b) => (
                    <div key={b.name} className="flex flex-col gap-1">
                      <div className="flex justify-between text-slate-700 dark:text-slate-300 capitalize">
                        <span className="font-medium">{b.name}</span>
                        <span className="font-mono">{b.count.toLocaleString()} ({b.percent}%)</span>
                      </div>
                      <div className="w-full h-2 bg-slate-100 dark:bg-slate-700 rounded-full overflow-hidden">
                        <div
                          className="h-full bg-indigo-600 rounded-full transition-all"
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
                <span>24 小时访问时段总热度分布</span>
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
                        {h.hour}:00 - {h.hour}:59 : {h.count} 次
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
          <div className="bg-gradient-to-r from-blue-50/70 via-indigo-50/70 to-purple-50/70 dark:from-blue-950/20 dark:via-indigo-950/20 dark:to-purple-950/20 p-4 rounded-xl border border-blue-200/60 dark:border-blue-800/40 shadow-sm flex flex-col gap-3">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div className="flex items-center gap-2.5">
                <div className="p-2 rounded-lg bg-blue-500/10 dark:bg-blue-400/10 text-blue-600 dark:text-blue-400">
                  <Brain className="w-5 h-5" />
                </div>
                <div>
                  <h3 className="font-semibold text-slate-800 dark:text-slate-100 text-sm flex items-center gap-2">
                    <span>AI 认知演化与时段对比洞察</span>
                    <span className="px-2 py-0.5 rounded-full text-[10px] bg-blue-100 dark:bg-blue-900/60 text-blue-700 dark:text-blue-300 font-medium">
                      双周期自适应
                    </span>
                  </h3>
                  <p className="text-slate-500 dark:text-slate-400 text-xs mt-0.5">
                    对比本期与上一周期的浏览偏好与主题迁移，深度解读个人关注重心的升降演进
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
                <span>{isGeneratingReport ? 'AI 正在深度比对演化...' : '生成本期演化洞察报告'}</span>
              </button>
            </div>

            {aiReport && (
              <div className="mt-2 p-3.5 bg-white/90 dark:bg-slate-900/90 rounded-lg border border-slate-200/80 dark:border-slate-800 shadow-sm">
                <div className="flex items-center justify-between pb-2 border-b border-slate-100 dark:border-slate-800 mb-2">
                  <span className="font-medium text-slate-700 dark:text-slate-200 flex items-center gap-1.5 text-xs">
                    <FileText className="w-3.5 h-3.5 text-blue-500" />
                    <span>AI 报告正文</span>
                  </span>
                  <button
                    onClick={() => {
                      navigator.clipboard.writeText(aiReport);
                      showToast('报告已复制到剪贴板', 'success');
                    }}
                    className="text-slate-400 hover:text-slate-600 dark:hover:text-slate-300 text-[11px] underline cursor-pointer"
                  >
                    复制报告
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
              <Loader2 className="w-4 h-4 animate-spin text-blue-500" />
              <span>正在加载分类体系与演化动态...</span>
            </div>
          ) : (
            <>
              {/* Evolution Dynamics (3 Columns: Rising, New, Declining) */}
              <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
                {/* Rising Topics */}
                <div className="bg-white dark:bg-slate-800 p-4 rounded-xl border border-emerald-100 dark:border-emerald-950/60 shadow-sm flex flex-col gap-3">
                  <div className="flex items-center justify-between pb-2 border-b border-slate-100 dark:border-slate-700/60">
                    <div className="flex items-center gap-2 font-semibold text-emerald-600 dark:text-emerald-400">
                      <TrendingUp className="w-4 h-4" />
                      <span>兴趣激增 (Rising)</span>
                    </div>
                    <span className="text-[11px] font-mono text-emerald-600/80 dark:text-emerald-400/80 bg-emerald-50 dark:bg-emerald-950/50 px-2 py-0.5 rounded-full">
                      {evolution?.rising_topics.length || 0} 个主题
                    </span>
                  </div>

                  <div className="flex flex-col gap-2">
                    {!evolution?.rising_topics.length ? (
                      <div className="py-8 text-center text-slate-400 text-xs">暂无激增主题</div>
                    ) : (
                      evolution.rising_topics.map((t) => (
                        <div
                          key={t.topic_id}
                          className="p-2.5 rounded-lg bg-emerald-50/50 dark:bg-emerald-950/20 border border-emerald-100/60 dark:border-emerald-900/40 flex items-center justify-between"
                        >
                          <div className="flex flex-col">
                            <span className="font-medium text-slate-800 dark:text-slate-100 text-xs">
                              {t.name}
                            </span>
                            <span className="text-[10px] text-slate-400">
                              前周期 {t.previous_visits} 次 → 本期 {t.current_visits} 次
                            </span>
                          </div>
                          <span className="text-xs font-semibold text-emerald-600 dark:text-emerald-400 font-mono">
                            +{t.growth_rate.toFixed(0)}%
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
                      <span>新兴探索 (New)</span>
                    </div>
                    <span className="text-[11px] font-mono text-blue-600/80 dark:text-blue-400/80 bg-blue-50 dark:bg-blue-950/50 px-2 py-0.5 rounded-full">
                      {evolution?.new_topics.length || 0} 个主题
                    </span>
                  </div>

                  <div className="flex flex-col gap-2">
                    {!evolution?.new_topics.length ? (
                      <div className="py-8 text-center text-slate-400 text-xs">本期暂无全新探索主题</div>
                    ) : (
                      evolution.new_topics.map((t) => (
                        <div
                          key={t.topic_id}
                          className="p-2.5 rounded-lg bg-blue-50/50 dark:bg-blue-950/20 border border-blue-100/60 dark:border-blue-900/40 flex items-center justify-between"
                        >
                          <div className="flex flex-col">
                            <span className="font-medium text-slate-800 dark:text-slate-100 text-xs">
                              {t.name}
                            </span>
                            <span className="text-[10px] text-slate-400">前周期未活跃</span>
                          </div>
                          <span className="text-xs font-semibold text-blue-600 dark:text-blue-400 font-mono">
                            {t.current_visits} 次访问
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
                      <span>关注降温 (Declining)</span>
                    </div>
                    <span className="text-[11px] font-mono text-slate-500 bg-slate-100 dark:bg-slate-700 px-2 py-0.5 rounded-full">
                      {evolution?.declining_topics.length || 0} 个主题
                    </span>
                  </div>

                  <div className="flex flex-col gap-2">
                    {!evolution?.declining_topics.length ? (
                      <div className="py-8 text-center text-slate-400 text-xs">暂无明显降温主题</div>
                    ) : (
                      evolution.declining_topics.map((t) => (
                        <div
                          key={t.topic_id}
                          className="p-2.5 rounded-lg bg-slate-50 dark:bg-slate-700/40 border border-slate-200 dark:border-slate-600/60 flex items-center justify-between"
                        >
                          <div className="flex flex-col">
                            <span className="font-medium text-slate-800 dark:text-slate-200 text-xs">
                              {t.name}
                            </span>
                            <span className="text-[10px] text-slate-400">
                              前周期 {t.previous_visits} 次 → 本期 {t.current_visits} 次
                            </span>
                          </div>
                          <span className="text-xs font-semibold text-slate-500 dark:text-slate-400 font-mono">
                            {t.growth_rate.toFixed(0)}%
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
                      全域主题分类谱系与活跃量 ({topics.length} 个类别)
                    </span>
                  </div>
                  <span className="text-slate-400 text-[11px]">
                    根据规则体系与关键词自动对历史页面进行归类
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
                        <span>累计访问: {top.visit_count} 次</span>
                        <span>{top.url_count} 网页</span>
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
              基于 30 分钟无活动自动切分的连续研究探索流 (共聚类 {sessions.length} 个会话)
            </span>
          </div>

          {isLoadingSessions ? (
            <div className="py-24 flex items-center justify-center text-slate-400 gap-2">
              <Loader2 className="w-4 h-4 animate-spin text-blue-500" />
              <span>正在切分与聚合研究会话...</span>
            </div>
          ) : sessions.length === 0 ? (
            <div className="py-20 text-center text-slate-400 text-xs">当前时段暂无聚类会话</div>
          ) : (
            <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
              {sessions.map((ses) => (
                <div
                  key={ses.session_id}
                  className="bg-white dark:bg-slate-800 p-3.5 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-2 hover:border-blue-300 dark:hover:border-blue-700 transition"
                >
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2 font-mono text-[11px] text-slate-400">
                      <Clock className="w-3 h-3 text-blue-500" />
                      <span>{new Date(ses.start_time).toLocaleString()}</span>
                    </div>
                    <span className="px-2 py-0.5 rounded-full text-[10px] bg-slate-100 dark:bg-slate-700 text-slate-600 dark:text-slate-300 font-medium">
                      时长 {formatDuration(ses.duration_secs)}
                    </span>
                  </div>

                  <div className="font-semibold text-slate-800 dark:text-slate-100 text-xs line-clamp-1">
                    {ses.sample_title || ses.dominant_domain}
                  </div>

                  <div className="flex items-center justify-between pt-1 border-t border-slate-100 dark:border-slate-700 text-[11px] text-slate-500">
                    <div className="flex items-center gap-1.5">
                      <Globe className="w-3 h-3 text-slate-400" />
                      <span className="font-mono text-slate-700 dark:text-slate-300 font-medium">
                        {ses.dominant_domain || '未知站点'}
                      </span>
                    </div>
                    <div className="flex items-center gap-2">
                      <span className="font-medium text-blue-600 dark:text-blue-400">
                        {ses.visit_count} 个页面
                      </span>
                      <span>·</span>
                      <span>{ses.browser}</span>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      ) : (
        /* On This Day View (历史上的今天) */
        <div className="flex flex-col gap-4 mt-5">
          {/* Subheader & Date Picker */}
          <div className="bg-white dark:bg-slate-800 p-4 rounded-xl border border-slate-200 dark:border-slate-700 shadow-sm flex flex-wrap items-center justify-between gap-3">
            <div className="flex items-center gap-3">
              <div className="p-2 rounded-lg bg-blue-50 dark:bg-blue-950/60 text-blue-600 dark:text-blue-400">
                <Calendar className="w-5 h-5" />
              </div>
              <div>
                <h3 className="font-semibold text-slate-800 dark:text-slate-100 text-sm flex items-center gap-2">
                  <span>往年今日的回忆足迹</span>
                  <span className="px-2 py-0.5 rounded-full text-[10px] bg-blue-100 dark:bg-blue-900/60 text-blue-700 dark:text-blue-300 font-medium font-mono">
                    {onThisDayResult?.total_count ?? 0} 条时光记忆
                  </span>
                </h3>
                <p className="text-slate-500 dark:text-slate-400 text-xs mt-0.5">
                  重温过去历年在同一天浏览过的内容，寻找往昔专注的思维轨迹与灵感
                </p>
              </div>
            </div>

            <div className="flex items-center gap-2">
              <span className="text-slate-500 text-xs">回忆日期:</span>
              <input
                type="date"
                value={onThisDayDate}
                onChange={(e) => {
                  const val = e.target.value;
                  setOnThisDayDate(val);
                  setCurrentPage(1);
                  fetchOnThisDayData(val, selectedYear, 1);
                }}
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
                回到今日
              </button>
            </div>
          </div>

          {/* Years Filter Pills */}
          {onThisDayResult && onThisDayResult.available_years && onThisDayResult.available_years.length > 0 && (
            <div className="flex items-center gap-2 flex-wrap bg-white dark:bg-slate-800 p-3 rounded-xl border border-slate-200 dark:border-slate-700 shadow-xs text-xs">
              <span className="text-slate-400 text-xs font-medium">历年筛选:</span>
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
                全部历年 ({onThisDayResult.total_count})
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
                  {yr} 年
                </button>
              ))}
            </div>
          )}

          {/* List of On This Day items */}
          {isLoadingOnThisDay ? (
            <div className="py-24 flex items-center justify-center text-slate-400 gap-2">
              <Loader2 className="w-4 h-4 animate-spin text-blue-500" />
              <span>正在寻回往昔记忆...</span>
            </div>
          ) : !onThisDayResult || onThisDayResult.items.length === 0 ? (
            <div className="bg-white dark:bg-slate-800 p-12 rounded-xl border border-slate-200 dark:border-slate-700 text-center flex flex-col items-center justify-center gap-2">
              <History className="w-8 h-8 text-slate-300 dark:text-slate-600" />
              <p className="text-slate-500 dark:text-slate-400 text-xs">
                在所选日期的往年中暂无归档浏览记录
              </p>
              <span className="text-slate-400 text-[11px]">
                尝试切换其他日期或导入更早的浏览器历史归档
              </span>
            </div>
          ) : (
            <>
              <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                {onThisDayResult.items.map((item) => (
                  <div
                    key={item.id}
                    className="bg-white dark:bg-slate-800 p-3.5 rounded-xl border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-2 hover:border-blue-300 dark:hover:border-blue-700 transition group"
                  >
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-1.5">
                        <span className="px-2 py-0.5 rounded-full text-[10px] font-semibold bg-amber-50 dark:bg-amber-950/60 text-amber-600 dark:text-amber-400 border border-amber-200/60 dark:border-amber-800/40">
                          {item.years_ago} 年前的今天
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
                          title="在历史记录中检索该域名"
                        >
                          <Search className="w-3.5 h-3.5" />
                        </button>
                        <button
                          type="button"
                          onClick={() => tauriApi.openExternalUrl(item.url)}
                          className="p-1 rounded text-slate-400 hover:text-blue-600 hover:bg-slate-100 dark:hover:bg-slate-700 transition cursor-pointer"
                          title="在默认浏览器中打开"
                        >
                          <ExternalLink className="w-3.5 h-3.5" />
                        </button>
                      </div>
                    </div>

                    <a
                      href={item.url}
                      target="_blank"
                      rel="noreferrer"
                      className="font-semibold text-slate-800 dark:text-slate-100 text-xs line-clamp-2 hover:text-blue-600 dark:hover:text-blue-400 transition-colors"
                    >
                      {item.title || item.url}
                    </a>

                    <div className="flex items-center justify-between pt-1.5 border-t border-slate-100 dark:border-slate-700/80 text-[11px] text-slate-500">
                      <div className="flex items-center gap-1.5 truncate pr-2">
                        <Globe className="w-3 h-3 text-slate-400 shrink-0" />
                        <span className="font-mono text-slate-600 dark:text-slate-300 font-medium truncate">
                          {item.domain || '未知域名'}
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
                    共 {onThisDayResult.total_count} 条往昔足迹 · 每页 30 条
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
                      上一页
                    </button>
                    <span className="text-slate-600 dark:text-slate-400 font-medium font-mono px-1">
                      第 {onThisDayResult.page} / {onThisDayResult.total_pages || 1} 页
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
                      下一页
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
