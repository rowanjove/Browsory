import React, { useEffect, useState } from 'react';
import {
  Activity,
  CheckCircle2,
  AlertTriangle,
  Download,
  RefreshCw,
  Server,
  Database,
  ShieldCheck,
  HardDrive,
  Cpu,
} from 'lucide-react';
import { tauriApi } from '../../services/tauri';
import { DiagnosticsInfo } from '../../types';
import { useAppStore } from '../../stores/useAppStore';

export const DiagnosticsCard: React.FC = () => {
  const { showToast } = useAppStore();
  const [loading, setLoading] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [diagnostics, setDiagnostics] = useState<DiagnosticsInfo | null>(null);

  const fetchDiagnostics = async () => {
    setLoading(true);
    try {
      const data = await tauriApi.getDiagnosticsInfo();
      setDiagnostics(data);
    } catch (err: any) {
      showToast('获取系统诊断信息失败: ' + (err?.message || err), 'error');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchDiagnostics();
  }, []);

  const handleExportBundle = async () => {
    setExporting(true);
    try {
      const zipPath = await tauriApi.exportDiagnosticsBundle();
      showToast(`诊断包已成功导出 (已完全脱敏): ${zipPath}`, 'success');
    } catch (err: any) {
      showToast('导出诊断包失败: ' + (err?.message || err), 'error');
    } finally {
      setExporting(false);
    }
  };

  const formatBytes = (bytes: number) => {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
  };

  return (
    <div className="bg-white dark:bg-slate-900 rounded-2xl border border-slate-200 dark:border-slate-800 shadow-xs p-6">
      <div className="flex items-center justify-between mb-4">
        <div className="flex items-center gap-3">
          <div className="p-2 bg-indigo-50 dark:bg-indigo-900/30 text-indigo-600 dark:text-indigo-400 rounded-lg">
            <Activity className="w-5 h-5" />
          </div>
          <div>
            <h2 className="text-base font-semibold text-slate-800 dark:text-slate-100 flex items-center gap-2">
              系统与数据库健康诊断
              <span className="text-xs px-2 py-0.5 rounded-full font-normal bg-indigo-100 dark:bg-indigo-950 text-indigo-700 dark:text-indigo-300">
                健康监测
              </span>
            </h2>
            <p className="text-xs text-slate-500 dark:text-slate-400">
              实时监控底层 SQLite 完整性、FTS 全文索引状态、系统硬件及各浏览器数据源采集状况
            </p>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={fetchDiagnostics}
            disabled={loading}
            className="flex items-center gap-1.5 px-3 py-1.5 text-xs text-slate-600 dark:text-slate-300 hover:bg-slate-100 dark:hover:bg-slate-700 rounded-lg border border-slate-200 dark:border-slate-600 transition-colors"
            title="刷新诊断指标"
          >
            <RefreshCw className={`w-3.5 h-3.5 ${loading ? 'animate-spin' : ''}`} />
            刷新状态
          </button>
          <button
            onClick={handleExportBundle}
            disabled={exporting || !diagnostics}
            className="flex items-center gap-1.5 px-3.5 py-1.5 text-xs bg-indigo-600 hover:bg-indigo-700 text-white rounded-lg font-medium shadow-sm transition-colors"
          >
            <Download className="w-3.5 h-3.5" />
            {exporting ? '正在打包脱敏中...' : '导出脱敏诊断包 (.zip)'}
          </button>
        </div>
      </div>

      {diagnostics ? (
        <div className="space-y-4">
          {/* Status Metrics Grid */}
          <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
            <div className="p-3 bg-slate-50 dark:bg-slate-900/50 rounded-lg border border-slate-200/80 dark:border-slate-700/80">
              <div className="flex items-center justify-between text-xs text-slate-500 dark:text-slate-400 mb-1">
                <span className="flex items-center gap-1">
                  <Database className="w-3.5 h-3.5 text-blue-500" /> 数据库完整性
                </span>
                {diagnostics.integrity_status.toLowerCase() === 'ok' ? (
                  <CheckCircle2 className="w-4 h-4 text-emerald-500" />
                ) : (
                  <AlertTriangle className="w-4 h-4 text-amber-500" />
                )}
              </div>
              <div className="text-sm font-semibold text-slate-800 dark:text-slate-200">
                {diagnostics.integrity_status.toLowerCase() === 'ok' ? 'Integrity OK' : diagnostics.integrity_status}
              </div>
              <div className="text-[10px] text-slate-400 mt-0.5">
                Schema v{diagnostics.db_schema_version} · 大小 {formatBytes(diagnostics.db_size_bytes)}
              </div>
            </div>

            <div className="p-3 bg-slate-50 dark:bg-slate-900/50 rounded-lg border border-slate-200/80 dark:border-slate-700/80">
              <div className="flex items-center justify-between text-xs text-slate-500 dark:text-slate-400 mb-1">
                <span className="flex items-center gap-1">
                  <HardDrive className="w-3.5 h-3.5 text-indigo-500" /> 索引记录规模
                </span>
                <CheckCircle2 className="w-4 h-4 text-emerald-500" />
              </div>
              <div className="text-sm font-semibold text-slate-800 dark:text-slate-200">
                {diagnostics.total_visits.toLocaleString()} 次访问
              </div>
              <div className="text-[10px] text-slate-400 mt-0.5">
                覆盖 {diagnostics.total_urls.toLocaleString()} 个独立 URL
              </div>
            </div>

            <div className="p-3 bg-slate-50 dark:bg-slate-900/50 rounded-lg border border-slate-200/80 dark:border-slate-700/80">
              <div className="flex items-center justify-between text-xs text-slate-500 dark:text-slate-400 mb-1">
                <span className="flex items-center gap-1">
                  <Server className="w-3.5 h-3.5 text-purple-500" /> FTS5 检索索引
                </span>
                {diagnostics.fts_status === 'ok' ? (
                  <CheckCircle2 className="w-4 h-4 text-emerald-500" />
                ) : (
                  <AlertTriangle className="w-4 h-4 text-amber-500" />
                )}
              </div>
              <div className="text-sm font-semibold text-slate-800 dark:text-slate-200">
                {diagnostics.fts_status === 'ok' ? 'FTS5 Online' : 'FTS 重建中'}
              </div>
              <div className="text-[10px] text-slate-400 mt-0.5">
                毫秒级全文匹配已就绪
              </div>
            </div>

            <div className="p-3 bg-slate-50 dark:bg-slate-900/50 rounded-lg border border-slate-200/80 dark:border-slate-700/80">
              <div className="flex items-center justify-between text-xs text-slate-500 dark:text-slate-400 mb-1">
                <span className="flex items-center gap-1">
                  <Cpu className="w-3.5 h-3.5 text-emerald-500" /> 运行平台环境
                </span>
              </div>
              <div className="text-sm font-semibold text-slate-800 dark:text-slate-200 truncate">
                {diagnostics.os_name} {diagnostics.os_version}
              </div>
              <div className="text-[10px] text-slate-400 mt-0.5">
                {diagnostics.arch} · 系统内存 {diagnostics.total_memory_mb} MB
              </div>
            </div>
          </div>

          {/* Sources Summary Table */}
          <div className="mt-3">
            <h3 className="text-xs font-medium text-slate-600 dark:text-slate-300 mb-2 flex items-center justify-between">
              <span>已注册浏览器数据源与同步健康 ({diagnostics.sources.length} 个)</span>
            </h3>
            <div className="max-h-40 overflow-y-auto border border-slate-200 dark:border-slate-700 rounded-lg">
              <table className="w-full text-xs text-left">
                <thead className="bg-slate-100 dark:bg-slate-900/80 text-slate-500 dark:text-slate-400 font-medium">
                  <tr>
                    <th className="px-3 py-2">浏览器</th>
                    <th className="px-3 py-2">配置文件 (Profile)</th>
                    <th className="px-3 py-2">状态</th>
                    <th className="px-3 py-2">最近同步记录</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-slate-100 dark:divide-slate-800">
                  {diagnostics.sources.map((s) => (
                    <tr key={s.id} className="hover:bg-slate-50 dark:hover:bg-slate-900/40">
                      <td className="px-3 py-1.5 font-medium text-slate-800 dark:text-slate-200 capitalize">
                        {s.browser}
                      </td>
                      <td className="px-3 py-1.5 text-slate-600 dark:text-slate-300">
                        {s.profile}
                      </td>
                      <td className="px-3 py-1.5">
                        <span
                          className={`inline-flex items-center px-2 py-0.5 rounded-full text-[10px] font-medium ${
                            s.enabled
                              ? 'bg-emerald-100 text-emerald-800 dark:bg-emerald-950 dark:text-emerald-300'
                              : 'bg-slate-100 text-slate-600 dark:bg-slate-800 dark:text-slate-400'
                          }`}
                        >
                          {s.enabled ? '已启用监控' : '已暂停'}
                        </span>
                      </td>
                      <td className="px-3 py-1.5 text-slate-500 dark:text-slate-400">
                        {s.last_visit_time > 0
                          ? new Date(s.last_visit_time).toLocaleString()
                          : '尚未同步'}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>

          {/* Privacy Disclaimer Banner */}
          <div className="flex items-start gap-2.5 p-3 bg-emerald-50 dark:bg-emerald-950/30 border border-emerald-200 dark:border-emerald-800/50 rounded-lg text-xs text-emerald-800 dark:text-emerald-300">
            <ShieldCheck className="w-4 h-4 flex-shrink-0 mt-0.5 text-emerald-600 dark:text-emerald-400" />
            <div>
              <p className="font-medium">隐私安全保障承诺 (Zero-Knowledge Diagnostics)</p>
              <p className="text-[11px] opacity-90 mt-0.5">
                诊断包仅提取系统配置、数据库文件结构统计与经过彻底正则表达式脱敏的运行日志。
                <strong>绝不包含</strong>任何真实浏览历史、URL、网页标题、搜索词、AI 对话、API Key 或 PIN 密钥。
              </p>
            </div>
          </div>
        </div>
      ) : (
        <div className="py-8 text-center text-xs text-slate-400">
          正在读取系统诊断指标...
        </div>
      )}
    </div>
  );
};
