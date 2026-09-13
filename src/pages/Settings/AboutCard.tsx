import React, { useState } from 'react';
import { ShieldCheck, Info, ExternalLink, ChevronDown, ChevronUp, Github, Database, Clock } from 'lucide-react';
import { AppLogo } from '../../components/Common/AppLogo';
import { tauriApi } from '../../services/tauri';

export const AboutCard: React.FC = () => {
  const [showChangelog, setShowChangelog] = useState(false);

  return (
    <div className="bg-white dark:bg-slate-800 p-5 rounded-lg border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-4">
      {/* 头部标题与版本 */}
      <div className="flex items-start justify-between">
        <div className="flex items-center gap-3">
          <AppLogo size={36} className="shrink-0 drop-shadow-sm" />
          <div className="flex flex-col">
            <div className="flex items-center gap-2">
              <span className="text-sm font-bold text-slate-800 dark:text-slate-100">Browsory 浏览足迹</span>
              <span className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-blue-100 text-blue-700 dark:bg-blue-900/60 dark:text-blue-300 font-semibold">
                v0.1.0
              </span>
              <span className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-slate-100 text-slate-600 dark:bg-slate-700 dark:text-slate-300">
                Apache-2.0
              </span>
            </div>
            <span className="text-[11px] text-slate-500 dark:text-slate-400 mt-0.5">
              本地优先的多浏览器历史记录归档与智能分析工具
            </span>
          </div>
        </div>

        <button
          type="button"
          onClick={() => tauriApi.openExternalUrl('https://github.com/rowanjove/browsory')}
          className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-md border border-slate-200 dark:border-slate-700 hover:bg-slate-100 dark:hover:bg-slate-700 text-slate-700 dark:text-slate-300 text-xs transition cursor-pointer"
        >
          <Github className="w-3.5 h-3.5" />
          <span>GitHub 仓库</span>
          <ExternalLink className="w-3 h-3 text-slate-400" />
        </button>
      </div>

      {/* 设计原则与核心优势 */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-2.5 text-[11px]">
        <div className="p-3 rounded-md bg-slate-50 dark:bg-slate-900/60 border border-slate-100 dark:border-slate-800 flex flex-col gap-1">
          <div className="flex items-center gap-1.5 font-medium text-slate-700 dark:text-slate-200">
            <ShieldCheck className="w-3.5 h-3.5 text-emerald-500 shrink-0" />
            <span>完全本地，离线可控</span>
          </div>
          <p className="text-slate-500 dark:text-slate-400 leading-relaxed">
            所有足迹数据只保存在本机的 SQLite 数据库中，不设云端同步，无数据采集与隐私上传。
          </p>
        </div>

        <div className="p-3 rounded-md bg-slate-50 dark:bg-slate-900/60 border border-slate-100 dark:border-slate-800 flex flex-col gap-1">
          <div className="flex items-center gap-1.5 font-medium text-slate-700 dark:text-slate-200">
            <Database className="w-3.5 h-3.5 text-blue-500 shrink-0" />
            <span>多浏览器自动聚合</span>
          </div>
          <p className="text-slate-500 dark:text-slate-400 leading-relaxed">
            支持 Chrome、Edge、Brave、Vivaldi 等主流浏览器，自动识别多 Profile 并实现统一快照导入与去重。
          </p>
        </div>

        <div className="p-3 rounded-md bg-slate-50 dark:bg-slate-900/60 border border-slate-100 dark:border-slate-800 flex flex-col gap-1">
          <div className="flex items-center gap-1.5 font-medium text-slate-700 dark:text-slate-200">
            <Clock className="w-3.5 h-3.5 text-violet-500 shrink-0" />
            <span>毫秒级 FTS5 检索</span>
          </div>
          <p className="text-slate-500 dark:text-slate-400 leading-relaxed">
            基于 SQLite 内置全文索引引擎，即使数万条访问记录也能瞬间完成标题与 URL 实时过滤。
          </p>
        </div>
      </div>

      {/* 更新说明折叠面板 */}
      <div className="border border-slate-200 dark:border-slate-700 rounded-md overflow-hidden text-xs">
        <button
          type="button"
          onClick={() => setShowChangelog(!showChangelog)}
          className="w-full px-3 py-2 bg-slate-50 dark:bg-slate-800/80 hover:bg-slate-100 dark:hover:bg-slate-700/50 flex items-center justify-between transition cursor-pointer text-slate-700 dark:text-slate-200 font-medium"
        >
          <div className="flex items-center gap-2">
            <Info className="w-3.5 h-3.5 text-blue-500" />
            <span>版本更新说明（v0.1.0 初始正式版）</span>
          </div>
          {showChangelog ? <ChevronUp className="w-4 h-4" /> : <ChevronDown className="w-4 h-4" />}
        </button>

        {showChangelog && (
          <div className="p-3.5 bg-white dark:bg-slate-900 space-y-3 text-[11px] leading-relaxed text-slate-600 dark:text-slate-300">
            <div>
              <span className="font-semibold text-slate-800 dark:text-slate-100">1. 多浏览器数据同步</span>
              <p className="text-slate-500 dark:text-slate-400 mt-0.5">
                支持系统主流 Chromium 内核浏览器的安装检测与数据源发现，采用只读临时快照复制机制，避免与正在运行的浏览器发生文件锁冲突。
              </p>
            </div>
            <div>
              <span className="font-semibold text-slate-800 dark:text-slate-100">2. 全文检索与历史时间线</span>
              <p className="text-slate-500 dark:text-slate-400 mt-0.5">
                支持按关键词、域名、浏览器类型与时间范围进行交叉筛选；提供历史足迹流水列表与访问详情抽屉查看。
              </p>
            </div>
            <div>
              <span className="font-semibold text-slate-800 dark:text-slate-100">3. 行为统计与可视化分析</span>
              <p className="text-slate-500 dark:text-slate-400 mt-0.5">
                内置访问量趋势分析、24小时时段分布热力图以及访问频次最高的顶级域名排行，直观展示日常网络冲浪与探索偏好。
              </p>
            </div>
            <div>
              <span className="font-semibold text-slate-800 dark:text-slate-100">4. 隐私安全保护</span>
              <p className="text-slate-500 dark:text-slate-400 mt-0.5">
                提供主密码 PIN 保护机制，支持离开锁屏、超时自动锁定与连续输错指数退避防护，确保本地敏感历史不被他人查阅。
              </p>
            </div>
            <div>
              <span className="font-semibold text-slate-800 dark:text-slate-100">5. 外部数据导入与归档导出</span>
              <p className="text-slate-500 dark:text-slate-400 mt-0.5">
                支持导入 Google Takeout 导出的历史记录 JSON 文件，支持外部历史 SQLite 文件拖拽导入，并可将足迹导出为 CSV 或 Excel 文件备用。
              </p>
            </div>
            <div>
              <span className="font-semibold text-slate-800 dark:text-slate-100">6. 本地与云端 AI 智能回忆</span>
              <p className="text-slate-500 dark:text-slate-400 mt-0.5">
                支持通过标准 OpenAI 接口或本地部署的 Ollama 运行大模型，基于本地检索到的相关历史上下文进行问答总结与回忆。
              </p>
            </div>
          </div>
        )}
      </div>
    </div>
  );
};
