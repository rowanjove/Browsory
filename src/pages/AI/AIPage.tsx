import React, { useEffect, useState } from 'react';
import {
  Sparkles,
  Bot,
  Loader2,
  AlertCircle,
  MessageSquare,
  FileText,
  ExternalLink,
  Clock,
  ArrowRight,
  Send,
  Search,
  Database,
  Layers,
  Repeat,
  RefreshCw,
  Compass,
  CheckCircle2,
} from 'lucide-react';
import { useHistoryStore } from '../../stores/useHistoryStore';
import { useAppStore } from '../../stores/useAppStore';
import { tauriApi } from '../../services/tauri';
import {
  EmbeddingIndexingStatus,
  HistoryFilter,
  HybridSearchResult,
  ResumeSuggestion,
  SimilarPageItem,
  VisitListItem,
  WebMemoryCitation,
} from '../../types';

export const AIPage: React.FC = () => {
  const { items } = useHistoryStore();
  const { showToast } = useAppStore();

  const [activeTab, setActiveTab] = useState<'chat' | 'hybrid' | 'resume' | 'summary' | 'index'>('chat');

  // Chat / Memory Q&A State
  const [chatQuestion, setChatQuestion] = useState('');
  const [chatAnswer, setChatAnswer] = useState<string | null>(null);
  const [chatCitations, setChatCitations] = useState<WebMemoryCitation[]>([]);
  const [isAsking, setIsAsking] = useState(false);

  // Hybrid Search State
  const [hybridQuery, setHybridQuery] = useState('');
  const [hybridResults, setHybridResults] = useState<HybridSearchResult[]>([]);
  const [isSearchingHybrid, setIsSearchingHybrid] = useState(false);

  // Similar Pages Drawer/Modal
  const [similarTarget, setSimilarTarget] = useState<{ urlId: number; title: string } | null>(null);
  const [similarPages, setSimilarPages] = useState<SimilarPageItem[]>([]);
  const [isLoadingSimilar, setIsLoadingSimilar] = useState(false);

  // Embedding Indexing State
  const [embeddingStatus, setEmbeddingStatus] = useState<EmbeddingIndexingStatus | null>(null);
  const [isIndexing, setIsIndexing] = useState(false);

  // Resume State
  const [resumeData, setResumeData] = useState<ResumeSuggestion | null>(null);
  const [isLoadingResume, setIsLoadingResume] = useState(false);

  // Summary State
  const [includeUrls, setIncludeUrls] = useState(false);
  const [summaryMode, setSummaryMode] = useState<'current' | 'today' | 'recent'>('current');
  const [aiResponse, setAiResponse] = useState<string | null>(null);
  const [isGenerating, setIsGenerating] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);

  const fetchEmbeddingStatus = async () => {
    try {
      const status = await tauriApi.getEmbeddingStatus();
      setEmbeddingStatus(status);
    } catch (e) {
      console.warn('获取向量索引状态失败:', e);
    }
  };

  useEffect(() => {
    fetchEmbeddingStatus();
  }, []);

  const handleAskSubmit = async (questionText?: string) => {
    const q = (questionText || chatQuestion).trim();
    if (!q) return;

    setIsAsking(true);
    setChatAnswer(null);
    setChatCitations([]);

    try {
      const res = await tauriApi.askWebMemory(q);
      setChatAnswer(res.answer);
      setChatCitations(res.citations);
    } catch (err: any) {
      const msg = typeof err === 'string' ? err : err?.message || '提问失败';
      showToast(msg, 'error');
    } finally {
      setIsAsking(false);
    }
  };

  const handleHybridSearch = async (e?: React.FormEvent) => {
    if (e) e.preventDefault();
    const q = hybridQuery.trim();
    if (!q) return;

    setIsSearchingHybrid(true);
    try {
      const results = await tauriApi.hybridSearch(q, 30);
      setHybridResults(results);
    } catch (err: any) {
      showToast(err?.message || '混合语义检索失败', 'error');
    } finally {
      setIsSearchingHybrid(false);
    }
  };

  const handleOpenSimilar = async (urlId: number, title: string) => {
    setSimilarTarget({ urlId, title });
    setIsLoadingSimilar(true);
    try {
      const pages = await tauriApi.getSimilarPages(urlId);
      setSimilarPages(pages);
    } catch (err: any) {
      showToast(err?.message || '获取相似页面失败', 'error');
    } finally {
      setIsLoadingSimilar(false);
    }
  };

  const handleGenerateEmbeddings = async () => {
    setIsIndexing(true);
    try {
      const count = await tauriApi.generateEmbeddingsBatch(undefined, 30);
      showToast(`已增量生成并保存 ${count} 个页面的嵌入向量`, 'success');
      await fetchEmbeddingStatus();
    } catch (err: any) {
      showToast(err?.message || '生成向量索引失败，请检查设置中的 AI 配置', 'error');
    } finally {
      setIsIndexing(false);
    }
  };

  const handleFetchResume = async () => {
    setIsLoadingResume(true);
    try {
      const res = await tauriApi.getResumeSuggestion();
      setResumeData(res);
    } catch (err: any) {
      const msg = typeof err === 'string' ? err : err?.message || '获取续研建议失败';
      showToast(msg, 'error');
    } finally {
      setIsLoadingResume(false);
    }
  };

  const filterRecordsByPrivacyRules = async (rawRecords: VisitListItem[]): Promise<VisitListItem[]> => {
    try {
      const rules = await tauriApi.listPrivacyRules();
      const activeRules = rules.filter((r) => r.enabled);
      if (activeRules.length === 0) return rawRecords;

      return rawRecords.filter((rec) => {
        const d = (rec.domain || '').toLowerCase();
        const u = (rec.url || '').toLowerCase();
        for (const rule of activeRules) {
          const p = rule.pattern.toLowerCase();
          if (p.startsWith('*.')) {
            const suffix = p.slice(2);
            if (d === suffix || d.endsWith(`.${suffix}`)) return false;
          } else if (d.includes(p) || u.includes(p)) {
            return false;
          }
        }
        return true;
      });
    } catch (e) {
      console.error('Failed to fetch privacy rules for AI filtering:', e);
      throw new Error('获取隐私规则失败，已终止 AI 数据汇总以保护隐私安全 (Fail-Closed)');
    }
  };

  const prepareHistoryRecords = async (mode: 'current' | 'today' | 'recent'): Promise<VisitListItem[]> => {
    let rawList: VisitListItem[] = [];
    if (mode === 'current') {
      rawList = items.slice(0, 150);
    } else {
      const now = new Date();
      const endOfDay = now.getTime();

      if (mode === 'today') {
        const startOfDay = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
        const filter: HistoryFilter = {
          start_time: startOfDay,
          end_time: endOfDay,
          limit: 150,
        };
        const res = await tauriApi.getHistoryPage(filter);
        rawList = res.items;
      } else {
        const start30Days = endOfDay - 30 * 24 * 60 * 60 * 1000;
        const filter: HistoryFilter = {
          start_time: start30Days,
          end_time: endOfDay,
          limit: 150,
        };
        const res = await tauriApi.getHistoryPage(filter);
        rawList = res.items;
      }
    }

    return filterRecordsByPrivacyRules(rawList);
  };

  const handleGenerate = async () => {
    setIsGenerating(true);
    setErrorMessage(null);
    setAiResponse(null);

    try {
      const records = await prepareHistoryRecords(summaryMode);
      if (records.length === 0) {
        throw new Error('未检索到符合条件的浏览记录');
      }

      const promptPayload = records
        .map((r, i) => {
          const time = new Date(r.visit_time).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
          const urlInfo = includeUrls ? ` (${r.url})` : '';
          return `${i + 1}. [${time}] ${r.title || '无标题'}${urlInfo} - ${r.domain}`;
        })
        .join('\n');

      const systemPrompt = `你是一个专业的个人知识管理助手。请针对用户提供的浏览器历史访问记录，按以下结构生成专业洞察报告：
1. 【核心关注主题与技术栈】：分类归纳用户在此期间最关心的技术、工具或话题；
2. 【关键认知与发现】：用户可能在解决什么具体问题或调研什么方向；
3. 【下一步建议】：针对用户的浏览探索给出延伸阅读或实践建议。
输出保持中文、分点清晰、干练高效。`;

      const prompt = `以下是用户的一段浏览器历史记录（共 ${records.length} 条）：\n\n${promptPayload}\n\n请输出结构化洞察报告：`;

      const resp = await tauriApi.callAiCompletion(prompt, systemPrompt);
      setAiResponse(resp);
    } catch (err: any) {
      setErrorMessage(err?.message || '生成失败，请检查 AI 接口或网络连接');
    } finally {
      setIsGenerating(false);
    }
  };

  return (
    <div className="flex-1 flex flex-col h-full bg-slate-50 dark:bg-slate-900 overflow-y-auto p-6 text-xs select-none">
      {/* Header */}
      <div className="flex items-center justify-between pb-4 border-b border-slate-200 dark:border-slate-800">
        <div className="flex items-center gap-2">
          <Sparkles className="w-5 h-5 text-indigo-500" />
          <h1 className="text-base font-semibold text-slate-800 dark:text-slate-100">
            历史分析与智能问答
          </h1>
        </div>

        {/* Tab Switcher */}
        <div className="flex items-center bg-slate-200/80 dark:bg-slate-800 p-0.5 rounded-lg text-xs">
          <button
            onClick={() => setActiveTab('chat')}
            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-md transition font-medium ${
              activeTab === 'chat'
                ? 'bg-white dark:bg-slate-700 text-indigo-600 dark:text-indigo-400 shadow-sm'
                : 'text-slate-600 dark:text-slate-400 hover:text-slate-900'
            }`}
          >
            <MessageSquare className="w-3.5 h-3.5" />
            <span>智能问答</span>
          </button>
          <button
            onClick={() => setActiveTab('hybrid')}
            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-md transition font-medium ${
              activeTab === 'hybrid'
                ? 'bg-white dark:bg-slate-700 text-indigo-600 dark:text-indigo-400 shadow-sm'
                : 'text-slate-600 dark:text-slate-400 hover:text-slate-900'
            }`}
          >
            <Search className="w-3.5 h-3.5" />
            <span>混合语义检索</span>
          </button>
          <button
            onClick={() => setActiveTab('resume')}
            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-md transition font-medium ${
              activeTab === 'resume'
                ? 'bg-white dark:bg-slate-700 text-indigo-600 dark:text-indigo-400 shadow-sm'
                : 'text-slate-600 dark:text-slate-400 hover:text-slate-900'
            }`}
          >
            <Compass className="w-3.5 h-3.5" />
            <span>断点续研</span>
          </button>
          <button
            onClick={() => setActiveTab('index')}
            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-md transition font-medium ${
              activeTab === 'index'
                ? 'bg-white dark:bg-slate-700 text-indigo-600 dark:text-indigo-400 shadow-sm'
                : 'text-slate-600 dark:text-slate-400 hover:text-slate-900'
            }`}
          >
            <Database className="w-3.5 h-3.5" />
            <span>向量索引库</span>
          </button>
          <button
            onClick={() => setActiveTab('summary')}
            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-md transition font-medium ${
              activeTab === 'summary'
                ? 'bg-white dark:bg-slate-700 text-indigo-600 dark:text-indigo-400 shadow-sm'
                : 'text-slate-600 dark:text-slate-400 hover:text-slate-900'
            }`}
          >
            <FileText className="w-3.5 h-3.5" />
            <span>历史总结</span>
          </button>
        </div>
      </div>

      {/* TAB 1: Grounded Memory Q&A */}
      {activeTab === 'chat' && (
        <div className="flex flex-col gap-4 mt-5 max-w-4xl">
          <div className="bg-white dark:bg-slate-800 p-4 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-3">
            <span className="font-semibold text-slate-800 dark:text-slate-100 flex items-center gap-2">
              <Bot className="w-4 h-4 text-indigo-500" />
              <span>基于历史记录提问</span>
            </span>
            <p className="text-slate-500 text-xs">
              系统将从本地历史中检索候选网页作为参考上下文，回答你的问题。
            </p>

            <form
              onSubmit={(e) => {
                e.preventDefault();
                handleAskSubmit();
              }}
              className="flex items-center gap-2"
            >
              <input
                type="text"
                value={chatQuestion}
                onChange={(e) => setChatQuestion(e.target.value)}
                placeholder="例如：我上周看的那篇关于 Rust 异步执行器和 Tokio 源码的文章叫什么？"
                className="flex-1 px-3 py-2 bg-slate-50 dark:bg-slate-900 border border-slate-300 dark:border-slate-700 rounded text-xs focus:outline-none focus:ring-1 focus:ring-indigo-500"
              />
              <button
                type="submit"
                disabled={isAsking || !chatQuestion.trim()}
                className="px-4 py-2 bg-indigo-600 hover:bg-indigo-700 text-white rounded font-medium flex items-center gap-1.5 transition disabled:opacity-50"
              >
                {isAsking ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Send className="w-3.5 h-3.5" />}
                <span>提问</span>
              </button>
            </form>

            {/* Quick Prompts */}
            <div className="flex flex-wrap gap-1.5 pt-1">
              <span className="text-slate-400 self-center">快捷启发：</span>
              {[
                '我最近看过的关于 Tauri v2 插件系统的内容',
                '上次我查的那个 SQLite WAL 锁问题怎么解决的？',
                '总结我最近在 GitHub 上关注的开源仓库',
              ].map((q) => (
                <button
                  key={q}
                  onClick={() => {
                    setChatQuestion(q);
                    handleAskSubmit(q);
                  }}
                  className="px-2 py-0.5 rounded bg-slate-100 dark:bg-slate-700/60 hover:bg-indigo-50 hover:text-indigo-600 text-slate-600 dark:text-slate-300 transition text-[11px]"
                >
                  {q}
                </button>
              ))}
            </div>
          </div>

          {/* Answer Card */}
          {chatAnswer && (
            <div className="bg-white dark:bg-slate-800 p-4 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-3">
              <div className="flex items-center justify-between pb-2 border-b border-slate-100 dark:border-slate-700">
                <span className="font-semibold text-slate-800 dark:text-slate-100 flex items-center gap-2">
                  <CheckCircle2 className="w-4 h-4 text-emerald-500" />
                  <span>记忆回答</span>
                </span>
                <button
                  onClick={() => {
                    navigator.clipboard.writeText(chatAnswer);
                    showToast('已复制回答内容', 'success');
                  }}
                  className="text-indigo-600 dark:text-indigo-400 hover:underline"
                >
                  复制回答
                </button>
              </div>
              <div className="text-xs text-slate-700 dark:text-slate-200 font-sans leading-relaxed whitespace-pre-wrap select-text">
                {chatAnswer}
              </div>

              {/* Citations list */}
              {chatCitations.length > 0 && (
                <div className="mt-3 pt-3 border-t border-slate-100 dark:border-slate-700 flex flex-col gap-2">
                  <span className="font-medium text-slate-500 text-[11px]">
                    引用历史证据链 ({chatCitations.length} 篇)：
                  </span>
                  <div className="grid grid-cols-1 md:grid-cols-2 gap-2">
                    {chatCitations.map((c, i) => (
                      <div
                        key={`${c.visit_id}-${i}`}
                        className="p-2 rounded bg-slate-50 dark:bg-slate-700/50 border border-slate-100 dark:border-slate-700 flex items-center justify-between gap-2"
                      >
                        <div className="flex flex-col gap-0.5 truncate">
                          <span className="font-medium text-slate-800 dark:text-slate-200 truncate">
                            [{i + 1}] {c.title || c.url}
                          </span>
                          <span className="text-[10px] text-slate-400 font-mono truncate">{c.domain}</span>
                        </div>
                        <button
                          onClick={() => tauriApi.openExternalUrl(c.url)}
                          className="p-1 hover:text-indigo-600 text-slate-400"
                          title="在浏览器中打开"
                        >
                          <ExternalLink className="w-3.5 h-3.5" />
                        </button>
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </div>
          )}
        </div>
      )}

      {/* TAB 2: Hybrid Semantic Search */}
      {activeTab === 'hybrid' && (
        <div className="flex flex-col gap-4 mt-5">
          <div className="bg-white dark:bg-slate-800 p-4 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-3">
            <div className="flex items-center justify-between">
              <span className="font-semibold text-slate-800 dark:text-slate-100 flex items-center gap-2">
                <Search className="w-4 h-4 text-indigo-500" />
                <span>混合语义检索 (Hybrid Semantic Search · RRF Fusion)</span>
              </span>
              <span className="text-[11px] text-slate-400 font-mono">
                BM25 全文分词 + 向量余弦相似度双轨排名
              </span>
            </div>

            <form onSubmit={handleHybridSearch} className="flex items-center gap-2">
              <input
                type="text"
                value={hybridQuery}
                onChange={(e) => setHybridQuery(e.target.value)}
                placeholder="输入语义搜索词（无需记住确切标题，描述你的印象即可）..."
                className="flex-1 px-3 py-2 bg-slate-50 dark:bg-slate-900 border border-slate-300 dark:border-slate-700 rounded text-xs focus:outline-none focus:ring-1 focus:ring-indigo-500"
              />
              <button
                type="submit"
                disabled={isSearchingHybrid || !hybridQuery.trim()}
                className="px-4 py-2 bg-indigo-600 hover:bg-indigo-700 text-white rounded font-medium flex items-center gap-1.5 transition disabled:opacity-50"
              >
                {isSearchingHybrid ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Search className="w-3.5 h-3.5" />}
                <span>混合检索</span>
              </button>
            </form>
          </div>

          {/* Results List */}
          {isSearchingHybrid ? (
            <div className="py-16 flex items-center justify-center text-slate-400 gap-2">
              <Loader2 className="w-4 h-4 animate-spin text-indigo-500" />
              <span>正在融合全文关键词与语义向量计算...</span>
            </div>
          ) : hybridResults.length > 0 ? (
            <div className="flex flex-col gap-2">
              <div className="flex items-center justify-between text-slate-400 text-[11px] px-1">
                <span>检索命中 {hybridResults.length} 条高相关网页记录</span>
                <span>按 RRF 倒数排名融合评分排序</span>
              </div>
              {hybridResults.map((item, idx) => (
                <div
                  key={item.url_id}
                  className="bg-white dark:bg-slate-800 p-3 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex items-center justify-between hover:border-indigo-300 dark:hover:border-indigo-600 transition group"
                >
                  <div className="flex items-center gap-3 truncate pr-3 flex-1">
                    <span className="font-mono text-slate-400 w-5 text-center shrink-0">#{idx + 1}</span>
                    <div className="flex flex-col gap-1 truncate flex-1">
                      <div className="flex items-center gap-2">
                        <span
                          className="font-semibold text-slate-800 dark:text-slate-100 group-hover:text-indigo-600 truncate cursor-pointer"
                          onClick={() => tauriApi.openExternalUrl(item.url)}
                        >
                          {item.title || item.url}
                        </span>
                        {item.fts_rank !== null && item.fts_rank !== undefined && (
                          <span className="px-1.5 py-0.2 rounded text-[9px] bg-blue-50 text-blue-600 dark:bg-blue-950/60 dark:text-blue-300 shrink-0">
                            FTS 词频 #{item.fts_rank + 1}
                          </span>
                        )}
                        {item.vec_rank !== null && item.vec_rank !== undefined && (
                          <span className="px-1.5 py-0.2 rounded text-[9px] bg-purple-50 text-purple-600 dark:bg-purple-950/60 dark:text-purple-300 shrink-0">
                            向量语义 #{item.vec_rank + 1} ({Math.round(item.similarity_score * 100)}%)
                          </span>
                        )}
                      </div>
                      <div className="flex items-center gap-2 text-[10px] text-slate-400 font-mono truncate">
                        <span>{item.domain}</span>
                        <span>·</span>
                        <span>累计访问 {item.visit_count} 次</span>
                        <span>·</span>
                        <span>{new Date(item.last_visit_time).toLocaleDateString()}</span>
                      </div>
                    </div>
                  </div>

                  <div className="flex items-center gap-2 shrink-0">
                    <button
                      onClick={() => handleOpenSimilar(item.url_id, item.title || item.url)}
                      className="px-2 py-1 rounded bg-indigo-50 hover:bg-indigo-100 text-indigo-600 dark:bg-indigo-950/60 dark:hover:bg-indigo-900 text-[10px] font-medium transition"
                    >
                      相似挖掘
                    </button>
                    <button
                      onClick={() => tauriApi.openExternalUrl(item.url)}
                      className="p-1 text-slate-400 hover:text-indigo-600"
                      title="打开网页"
                    >
                      <ExternalLink className="w-3.5 h-3.5" />
                    </button>
                  </div>
                </div>
              ))}
            </div>
          ) : hybridQuery ? (
            <div className="py-12 text-center text-slate-400">未检索到相关记录，尝试更换描述或增量建立向量索引</div>
          ) : null}
        </div>
      )}

      {/* TAB 3: Resume Research */}
      {activeTab === 'resume' && (
        <div className="flex flex-col gap-4 mt-5 max-w-3xl">
          <div className="bg-white dark:bg-slate-800 p-4 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-3">
            <div className="flex items-center justify-between">
              <span className="font-semibold text-slate-800 dark:text-slate-100 flex items-center gap-2">
                <Compass className="w-4 h-4 text-blue-500" />
                <span>断点续研</span>
              </span>
              <button
                onClick={handleFetchResume}
                disabled={isLoadingResume}
                className="px-3 py-1 bg-blue-600 hover:bg-blue-700 text-white rounded font-medium flex items-center gap-1.5 transition disabled:opacity-50 text-[11px] cursor-pointer"
              >
                {isLoadingResume ? <Loader2 className="w-3 h-3 animate-spin" /> : <RefreshCw className="w-3 h-3" />}
                <span>分析最近研究会话</span>
              </button>
            </div>
            <p className="text-slate-500">
              根据最近一次连续探索的研究会话，分析当时关注的议题，并给出进一步阅读参考。
            </p>
          </div>

          {resumeData && (
            <div className="bg-white dark:bg-slate-800 p-4 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-4">
              <div className="flex items-center justify-between pb-2 border-b border-slate-100 dark:border-slate-700">
                <div className="flex items-center gap-2">
                  <Clock className="w-4 h-4 text-blue-500" />
                  <span className="font-semibold text-slate-800 dark:text-slate-100">
                    {new Date(resumeData.session.start_time).toLocaleString()} 的研究议题
                  </span>
                </div>
                <span className="px-2 py-0.5 rounded bg-blue-50 text-blue-600 dark:bg-blue-950/60 dark:text-blue-300 font-mono text-[10px]">
                  共 {resumeData.session.visit_count} 个页面 · {resumeData.session.dominant_domain}
                </span>
              </div>

              <div className="text-xs text-slate-700 dark:text-slate-200 leading-relaxed font-sans">
                {resumeData.summary}
              </div>

              <div className="flex flex-col gap-2 pt-2 border-t border-slate-100 dark:border-slate-700">
                <span className="font-medium text-slate-600 dark:text-slate-300 text-[11px]">
                  建议后续参考：
                </span>
                <div className="flex flex-col gap-1.5">
                  {resumeData.next_steps.map((step, i) => (
                    <div
                      key={i}
                      className="p-2 rounded bg-slate-50 dark:bg-slate-700/50 border border-slate-100 dark:border-slate-700 flex items-center gap-2"
                    >
                      <ArrowRight className="w-3.5 h-3.5 text-blue-500 shrink-0" />
                      <span className="text-slate-700 dark:text-slate-200">{step}</span>
                    </div>
                  ))}
                </div>
              </div>
            </div>
          )}
        </div>
      )}

      {/* TAB 4: Embedding Index Library */}
      {activeTab === 'index' && (
        <div className="flex flex-col gap-4 mt-5 max-w-3xl">
          <div className="bg-white dark:bg-slate-800 p-4 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-4">
            <div className="flex items-center justify-between pb-2 border-b border-slate-100 dark:border-slate-700">
              <span className="font-semibold text-slate-800 dark:text-slate-100 flex items-center gap-2">
                <Database className="w-4 h-4 text-purple-500" />
                <span>语义向量库与索引管理 (Semantic Vector Index)</span>
              </span>
              <button
                onClick={fetchEmbeddingStatus}
                className="p-1 text-slate-400 hover:text-slate-700 dark:hover:text-slate-200"
                title="刷新状态"
              >
                <RefreshCw className="w-3.5 h-3.5" />
              </button>
            </div>

            <div className="grid grid-cols-3 gap-3">
              <div className="p-3 rounded bg-slate-50 dark:bg-slate-700/50 flex flex-col gap-1">
                <span className="text-slate-400 text-[11px]">历史唯一网页总数</span>
                <span className="text-lg font-bold font-mono text-slate-800 dark:text-slate-100">
                  {embeddingStatus?.total_urls ?? 0}
                </span>
              </div>
              <div className="p-3 rounded bg-purple-50 dark:bg-purple-950/40 flex flex-col gap-1">
                <span className="text-purple-600 dark:text-purple-300 text-[11px]">已完成向量索引</span>
                <span className="text-lg font-bold font-mono text-purple-700 dark:text-purple-200">
                  {embeddingStatus?.indexed_urls ?? 0}
                </span>
              </div>
              <div className="p-3 rounded bg-amber-50 dark:bg-amber-950/40 flex flex-col gap-1">
                <span className="text-amber-600 dark:text-amber-300 text-[11px]">待索引网页</span>
                <span className="text-lg font-bold font-mono text-amber-700 dark:text-amber-200">
                  {embeddingStatus?.pending_urls ?? 0}
                </span>
              </div>
            </div>

            <div className="flex items-center justify-between pt-2 border-t border-slate-100 dark:border-slate-700">
              <div className="flex flex-col gap-0.5 text-[11px] text-slate-500">
                <span>默认模型：{embeddingStatus?.model || 'text-embedding-3-small'}</span>
                <span>支持在“系统设置”中配置本地 Ollama 或 OpenAI 兼容向量端点</span>
              </div>

              <button
                onClick={handleGenerateEmbeddings}
                disabled={isIndexing || (embeddingStatus?.pending_urls || 0) === 0}
                className="px-4 py-2 bg-purple-600 hover:bg-purple-700 text-white rounded font-medium flex items-center gap-2 transition disabled:opacity-50 shadow-sm"
              >
                {isIndexing ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Layers className="w-3.5 h-3.5" />}
                <span>{isIndexing ? '正在批量生成向量嵌入...' : '一键增量生成向量索引'}</span>
              </button>
            </div>
          </div>
        </div>
      )}

      {/* TAB 5: AI Summarizer */}
      {activeTab === 'summary' && (
        <div className="flex flex-col gap-4 mt-5 max-w-3xl">
          <div className="p-3 bg-purple-50/50 dark:bg-purple-950/20 border border-purple-100 dark:border-purple-900/50 rounded flex flex-col gap-1">
            <span className="font-semibold text-purple-900 dark:text-purple-300">
              数据范围与隐私策略
            </span>
            <p className="text-purple-700/80 dark:text-purple-400">
              系统将严格遵守安全配置，自动过滤内网、本地及隐私规则标记的域名，仅提取代表性记录供模型归纳。
            </p>
            <label className="flex items-center gap-2 cursor-pointer mt-1">
              <input
                type="checkbox"
                checked={includeUrls}
                onChange={(e) => setIncludeUrls(e.target.checked)}
                className="rounded border-slate-300 text-purple-600 focus:ring-0"
              />
              <span className="text-slate-700 dark:text-slate-300">
                包含完整 URL（关闭时仅发送访问时间和网页标题，增强隐私保护）
              </span>
            </label>
          </div>

          <div className="flex gap-2">
            <button
              onClick={() => setSummaryMode('current')}
              className={`flex-1 py-2.5 px-3 rounded border text-left flex flex-col gap-1 transition ${
                summaryMode === 'current'
                  ? 'border-purple-500 bg-white dark:bg-slate-800 text-purple-600 dark:text-purple-400 font-medium'
                  : 'border-slate-200 dark:border-slate-700 bg-white dark:bg-slate-800 text-slate-600'
              }`}
            >
              <span>分析当前筛选结果</span>
              <span className="text-[10px] opacity-75">针对历史页当前搜索与筛选的项目深入总结</span>
            </button>
            <button
              onClick={() => setSummaryMode('today')}
              className={`flex-1 py-2.5 px-3 rounded border text-left flex flex-col gap-1 transition ${
                summaryMode === 'today'
                  ? 'border-purple-500 bg-white dark:bg-slate-800 text-purple-600 dark:text-purple-400 font-medium'
                  : 'border-slate-200 dark:border-slate-700 bg-white dark:bg-slate-800 text-slate-600'
              }`}
            >
              <span>今日浏览速览</span>
              <span className="text-[10px] opacity-75">总结今天从早到晚的浏览轨迹与重点内容</span>
            </button>
            <button
              onClick={() => setSummaryMode('recent')}
              className={`flex-1 py-2.5 px-3 rounded border text-left flex flex-col gap-1 transition ${
                summaryMode === 'recent'
                  ? 'border-purple-500 bg-white dark:bg-slate-800 text-purple-600 dark:text-purple-400 font-medium'
                  : 'border-slate-200 dark:border-slate-700 bg-white dark:bg-slate-800 text-slate-600'
              }`}
            >
              <span>最近30天主题提炼</span>
              <span className="text-[10px] opacity-75">提炼近一个月主要的研究方向与知识焦点</span>
            </button>
          </div>

          <button
            onClick={handleGenerate}
            disabled={isGenerating}
            className="self-start flex items-center gap-2 px-4 py-2 bg-purple-600 hover:bg-purple-700 text-white rounded font-medium transition shadow-sm disabled:opacity-50"
          >
            {isGenerating ? <Loader2 className="w-4 h-4 animate-spin" /> : <Bot className="w-4 h-4" />}
            <span>{isGenerating ? 'AI 正在分析提炼中...' : '开始 AI 总结'}</span>
          </button>

          {errorMessage && (
            <div className="p-3 rounded bg-rose-50 border border-rose-200 text-rose-700 flex items-start gap-2">
              <AlertCircle className="w-4 h-4 shrink-0 mt-0.5" />
              <span>{errorMessage}</span>
            </div>
          )}

          {aiResponse && (
            <div className="p-4 bg-white dark:bg-slate-800 border border-slate-200 dark:border-slate-700 rounded shadow-sm flex flex-col gap-2">
              <div className="flex items-center justify-between pb-2 border-b border-slate-100 dark:border-slate-700">
                <span className="font-semibold text-slate-800 dark:text-slate-100 flex items-center gap-1.5">
                  <Sparkles className="w-3.5 h-3.5 text-purple-500" />
                  <span>AI 分析总结报告</span>
                </span>
                <button
                  onClick={() => {
                    navigator.clipboard.writeText(aiResponse);
                    showToast('已复制 AI 报告至剪贴板', 'success');
                  }}
                  className="text-purple-600 dark:text-purple-400 hover:underline"
                >
                  复制全文
                </button>
              </div>
              <pre className="text-xs text-slate-700 dark:text-slate-200 font-sans whitespace-pre-wrap leading-relaxed select-text pt-1">
                {aiResponse}
              </pre>
            </div>
          )}
        </div>
      )}

      {/* Similar Pages Modal */}
      {similarTarget && (
        <div className="fixed inset-0 z-50 bg-slate-900/50 backdrop-blur-xs flex items-center justify-center p-4">
          <div className="bg-white dark:bg-slate-800 rounded-lg shadow-xl border border-slate-200 dark:border-slate-700 w-full max-w-2xl max-h-[80vh] flex flex-col overflow-hidden">
            <div className="flex items-center justify-between p-4 border-b border-slate-200 dark:border-slate-700">
              <div className="flex items-center gap-2">
                <Repeat className="w-4 h-4 text-indigo-500" />
                <span className="font-semibold text-slate-800 dark:text-slate-100 truncate max-w-md">
                  相似历史网页挖掘: {similarTarget.title}
                </span>
              </div>
              <button
                onClick={() => setSimilarTarget(null)}
                className="text-slate-400 hover:text-slate-700 dark:hover:text-slate-200"
              >
                ✕
              </button>
            </div>

            <div className="p-4 overflow-y-auto flex flex-col gap-2">
              {isLoadingSimilar ? (
                <div className="py-12 flex items-center justify-center text-slate-400 gap-2">
                  <Loader2 className="w-4 h-4 animate-spin text-indigo-500" />
                  <span>正在比对全量向量余弦相似度...</span>
                </div>
              ) : similarPages.length === 0 ? (
                <div className="py-8 text-center text-slate-400">未检索到相似度高于 0.25 的其他历史页面</div>
              ) : (
                similarPages.map((sp) => (
                  <div
                    key={sp.url_id}
                    className="p-3 rounded border border-slate-100 dark:border-slate-700 hover:bg-slate-50 dark:hover:bg-slate-700/50 flex items-center justify-between transition group"
                  >
                    <div className="flex flex-col gap-0.5 truncate pr-2 flex-1">
                      <span className="font-medium text-slate-800 dark:text-slate-200 group-hover:text-indigo-600 truncate">
                        {sp.title || sp.url}
                      </span>
                      <div className="flex items-center gap-2 text-[10px] text-slate-400 font-mono">
                        <span>{sp.domain}</span>
                        <span>·</span>
                        <span>访问 {sp.visit_count} 次</span>
                        <span>·</span>
                        <span>上次 {new Date(sp.last_visit_time).toLocaleDateString()}</span>
                      </div>
                    </div>
                    <div className="flex items-center gap-2 shrink-0">
                      <span className="px-2 py-0.5 rounded bg-purple-50 text-purple-600 dark:bg-purple-950/60 dark:text-purple-300 font-mono text-[10px] font-semibold">
                        相似度 {Math.round(sp.similarity * 100)}%
                      </span>
                      <button
                        onClick={() => tauriApi.openExternalUrl(sp.url)}
                        className="p-1 text-slate-400 hover:text-indigo-600"
                        title="打开网页"
                      >
                        <ExternalLink className="w-3.5 h-3.5" />
                      </button>
                    </div>
                  </div>
                ))
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
};
