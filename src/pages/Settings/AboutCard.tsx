import React, { useState } from 'react';
import {
  ShieldCheck,
  ExternalLink,
  Github,
  Database,
  Clock,
  Info,
  RefreshCw,
} from 'lucide-react';
import { AppLogo } from '../../components/Common/AppLogo';
import { tauriApi } from '../../services/tauri';
import { useAppStore } from '../../stores/useAppStore';

export const AboutCard: React.FC = () => {
  const { openChangelog, appVersion, showToast } = useAppStore();
  const [checkingUpdate, setCheckingUpdate] = useState(false);

  return (
    <div className="p-6 rounded-2xl border border-slate-200 dark:border-slate-800 bg-white dark:bg-slate-900 shadow-xs space-y-5">
      {/* 头部标题与版本 */}
      <div className="flex flex-col md:flex-row md:items-center justify-between gap-4">
        <div className="flex items-center gap-3.5">
          <AppLogo size={42} className="shrink-0 drop-shadow-sm" />
          <div className="flex flex-col">
            <div className="flex items-center gap-2 flex-wrap">
              <span className="text-base font-bold text-slate-900 dark:text-slate-100">
                Browsory 浏览足迹
              </span>
              <span className="text-xs font-mono px-2 py-0.5 rounded-full bg-blue-100 text-blue-700 dark:bg-blue-950/80 dark:text-blue-300 font-semibold border border-blue-200 dark:border-blue-800">
                v{appVersion}
              </span>
              <span className="text-xs font-mono px-2 py-0.5 rounded-full bg-slate-100 text-slate-600 dark:bg-slate-800 dark:text-slate-300 border border-slate-200 dark:border-slate-700">
                Apache-2.0
              </span>
            </div>
            <span className="text-xs text-slate-500 dark:text-slate-400 mt-1">
              本地优先的多浏览器历史记录归档与智能分析工具
            </span>
          </div>
        </div>

        <div className="flex items-center gap-2.5 shrink-0">
          <button
            type="button"
            onClick={async () => {
              setCheckingUpdate(true);
              try {
                const result = await tauriApi.checkForUpdates();
                if (result.update_available && result.latest_version) {
                  showToast(`发现新版本 ${result.latest_version}，正在打开发布页`, 'success');
                  if (result.release_url) {
                    await tauriApi.openExternalUrl(result.release_url);
                  }
                } else {
                  showToast(`当前已是最新版本 v${result.current_version}`, 'info');
                }
              } catch (err: any) {
                showToast(typeof err === 'string' ? err : '检查更新失败', 'error');
              } finally {
                setCheckingUpdate(false);
              }
            }}
            disabled={checkingUpdate}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl border border-slate-200 dark:border-slate-700 hover:bg-slate-100 dark:hover:bg-slate-800 text-slate-700 dark:text-slate-300 text-xs font-medium transition cursor-pointer disabled:opacity-50"
          >
            <RefreshCw className={`w-3.5 h-3.5 ${checkingUpdate ? 'animate-spin' : ''}`} />
            <span>{checkingUpdate ? '检查中...' : '检查更新'}</span>
          </button>

          <button
            type="button"
            onClick={openChangelog}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl border border-blue-200 dark:border-blue-800 bg-blue-50/60 dark:bg-blue-950/40 hover:bg-blue-100 dark:hover:bg-blue-900/60 text-blue-700 dark:text-blue-300 text-xs font-medium transition cursor-pointer"
          >
            <Info className="w-3.5 h-3.5" />
            <span>版本更新说明</span>
          </button>

          <button
            type="button"
            onClick={() => tauriApi.openExternalUrl('https://github.com/rowanjove/browsory')}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl border border-slate-200 dark:border-slate-700 hover:bg-slate-100 dark:hover:bg-slate-800 text-slate-700 dark:text-slate-300 text-xs font-medium transition cursor-pointer"
          >
            <Github className="w-3.5 h-3.5" />
            <span>GitHub 仓库</span>
            <ExternalLink className="w-3 h-3 text-slate-400" />
          </button>
        </div>
      </div>

      {/* 设计原则与核心优势 */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-3 text-xs">
        <div className="p-4 rounded-xl bg-slate-50/80 dark:bg-slate-850/60 border border-slate-100 dark:border-slate-800/80 flex flex-col justify-between gap-2">
          <div className="space-y-1.5">
            <div className="flex items-center gap-2 font-semibold text-slate-800 dark:text-slate-200">
              <ShieldCheck className="w-4 h-4 text-emerald-500 shrink-0" />
              <span>完全本地，离线可控</span>
            </div>
            <p className="text-slate-500 dark:text-slate-400 leading-relaxed text-[11px]">
              所有足迹数据只保存在本机的 SQLite 数据库中，不设云端同步，无数据采集与隐私上传。
            </p>
          </div>
          <span className="text-[10px] text-emerald-600 dark:text-emerald-400 font-medium">
            Local-First 隐私基石
          </span>
        </div>

        <div className="p-4 rounded-xl bg-slate-50/80 dark:bg-slate-850/60 border border-slate-100 dark:border-slate-800/80 flex flex-col justify-between gap-2">
          <div className="space-y-1.5">
            <div className="flex items-center gap-2 font-semibold text-slate-800 dark:text-slate-200">
              <Database className="w-4 h-4 text-blue-500 shrink-0" />
              <span>多浏览器自动聚合</span>
            </div>
            <p className="text-slate-500 dark:text-slate-400 leading-relaxed text-[11px]">
              支持 Chrome、Edge、Brave、Vivaldi 等主流浏览器，自动识别多 Profile 并实现统一快照导入与去重。
            </p>
          </div>
          <span className="text-[10px] text-blue-600 dark:text-blue-400 font-medium">
            全域足迹统一管理
          </span>
        </div>

        <div className="p-4 rounded-xl bg-slate-50/80 dark:bg-slate-850/60 border border-slate-100 dark:border-slate-800/80 flex flex-col justify-between gap-2">
          <div className="space-y-1.5">
            <div className="flex items-center gap-2 font-semibold text-slate-800 dark:text-slate-200">
              <Clock className="w-4 h-4 text-purple-500 shrink-0" />
              <span>毫秒级 FTS5 检索</span>
            </div>
            <p className="text-slate-500 dark:text-slate-400 leading-relaxed text-[11px]">
              基于 SQLite 内置全文索引引擎，即使数万条访问记录也能瞬间完成标题与 URL 实时过滤。
            </p>
          </div>
          <span className="text-[10px] text-purple-600 dark:text-purple-400 font-medium">
            极速全文索引
          </span>
        </div>
      </div>
    </div>
  );
};
