import React, { useEffect } from 'react';
import {
  X,
  Sparkles,
  ExternalLink,
  ShieldCheck,
  Globe,
  Search,
  Lock,
  FileSpreadsheet,
  Brain,
  Github,
} from 'lucide-react';
import { AppLogo } from '../Common/AppLogo';
import { useAppStore } from '../../stores/useAppStore';
import { tauriApi } from '../../services/tauri';

export const ChangelogModal: React.FC = () => {
  const { isChangelogOpen, closeChangelog } = useAppStore();

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && isChangelogOpen) {
        closeChangelog();
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isChangelogOpen, closeChangelog]);

  if (!isChangelogOpen) return null;

  const features = [
    {
      icon: <Globe className="w-4 h-4 text-blue-500" />,
      title: '多浏览器自动聚合与安全导入',
      description:
        '支持 Chrome、Edge、Brave、Vivaldi、Opera、Zen 等系统主流浏览器的自动识别与多 Profile 聚合；采用只读临时快照复制机制，避免与运行中的浏览器发生文件锁冲突。',
    },
    {
      icon: <Search className="w-4 h-4 text-indigo-500" />,
      title: '毫秒级 FTS5 全文检索与历史时间线',
      description:
        '基于 SQLite 内置全文索引引擎，数万条访问记录也能瞬间完成标题与 URL 实时过滤；支持按关键词、域名、浏览器类型与时间范围交叉检索，并提供前后浏览时序还原。',
    },
    {
      icon: <Sparkles className="w-4 h-4 text-emerald-500" />,
      title: '行为统计与 24 小时访问热力图',
      description:
        '内置每日访问量趋势柱状图、24 小时生活节奏热力矩阵以及访问频次最高的顶级域名深度排行，直观复盘日常网络冲浪与探索偏好。',
    },
    {
      icon: <Lock className="w-4 h-4 text-amber-500" />,
      title: '本地优先隐私安全与应用访问锁',
      description:
        '数据 100% 存储于本机 SQLite 数据库，无隐蔽云端上传；提供 4-6 位数字 PIN 码安全锁、离开锁屏、超时自动锁定与连续输错指数退避防护。',
    },
    {
      icon: <FileSpreadsheet className="w-4 h-4 text-cyan-500" />,
      title: '外部数据导入与多格式归档导出',
      description:
        '支持导入 Google Takeout 导出的历史记录 JSON 文件与外部 SQLite 文件拖拽导入，并可将足迹导出为 CSV、JSON 或 Excel (.xlsx) 文件进行备份与迁移。',
    },
    {
      icon: <Brain className="w-4 h-4 text-purple-500" />,
      title: '本地与云端 AI 智能回忆与语义问答',
      description:
        '支持通过 OpenAI 兼容接口或本地部署的 Ollama / LM Studio 离线模型，结合历史记录上下文与向量嵌入（Embedding）实现智能问答、断点续研与主题聚类。',
    },
  ];

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/60 backdrop-blur-sm p-4 text-xs select-none animate-in fade-in duration-150">
      <div
        className="w-full max-w-2xl max-h-[85vh] bg-white dark:bg-slate-900 border border-slate-200 dark:border-slate-800 rounded-2xl shadow-2xl flex flex-col overflow-hidden animate-in zoom-in-95 duration-150"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="px-6 py-4 border-b border-slate-100 dark:border-slate-800 bg-slate-50/50 dark:bg-slate-850/50 flex items-center justify-between shrink-0">
          <div className="flex items-center gap-3">
            <AppLogo size={32} className="shrink-0 drop-shadow-sm" />
            <div>
              <div className="flex items-center gap-2">
                <h2 className="text-sm font-bold text-slate-900 dark:text-slate-100">
                  版本更新说明
                </h2>
                <span className="px-2 py-0.5 rounded-full text-[10px] font-semibold font-mono bg-blue-100 dark:bg-blue-950/80 text-blue-700 dark:text-blue-300 border border-blue-200 dark:border-blue-800">
                  v0.1.0 初始正式版
                </span>
              </div>
              <p className="text-[11px] text-slate-500 dark:text-slate-400 mt-0.5">
                Browsory · 本地优先的多浏览器历史记录归档与智能分析工具
              </p>
            </div>
          </div>

          <button
            type="button"
            onClick={closeChangelog}
            className="p-1.5 rounded-lg text-slate-400 hover:text-slate-600 dark:hover:text-slate-200 hover:bg-slate-100 dark:hover:bg-slate-800 transition cursor-pointer"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Content List */}
        <div className="flex-1 overflow-y-auto p-6 space-y-4">
          <div className="grid grid-cols-1 gap-3">
            {features.map((feat, idx) => (
              <div
                key={idx}
                className="p-3.5 rounded-xl bg-slate-50/70 dark:bg-slate-850/50 border border-slate-100 dark:border-slate-800/80 flex items-start gap-3.5"
              >
                <div className="p-2 rounded-lg bg-white dark:bg-slate-800 border border-slate-200/60 dark:border-slate-700 shadow-2xs shrink-0 mt-0.5">
                  {feat.icon}
                </div>
                <div className="flex flex-col gap-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="text-xs font-semibold text-slate-800 dark:text-slate-100">
                      {idx + 1}. {feat.title}
                    </span>
                  </div>
                  <p className="text-[11px] text-slate-500 dark:text-slate-400 leading-relaxed">
                    {feat.description}
                  </p>
                </div>
              </div>
            ))}
          </div>
        </div>

        {/* Footer */}
        <div className="px-6 py-3.5 border-t border-slate-100 dark:border-slate-800 bg-slate-50/50 dark:bg-slate-850/50 flex items-center justify-between shrink-0">
          <div className="flex items-center gap-3 text-[11px] text-slate-400">
            <span className="flex items-center gap-1">
              <ShieldCheck className="w-3.5 h-3.5 text-emerald-500" />
              <span>Apache-2.0 开源协议</span>
            </span>
          </div>

          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => tauriApi.openExternalUrl('https://github.com/rowanjove/browsory')}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-slate-200 dark:border-slate-700 hover:bg-white dark:hover:bg-slate-800 text-slate-700 dark:text-slate-300 transition cursor-pointer text-xs font-medium"
            >
              <Github className="w-3.5 h-3.5" />
              <span>GitHub 仓库</span>
              <ExternalLink className="w-3 h-3 text-slate-400" />
            </button>

            <button
              type="button"
              onClick={closeChangelog}
              className="px-4 py-1.5 rounded-lg bg-blue-600 hover:bg-blue-700 text-white font-medium transition cursor-pointer text-xs shadow-xs"
            >
              知道了
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};
