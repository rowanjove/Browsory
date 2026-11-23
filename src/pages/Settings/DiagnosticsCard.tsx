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
  ChevronDown,
  ChevronUp,
} from 'lucide-react';
import { tauriApi } from '../../services/tauri';
import { DiagnosticsInfo } from '../../types';
import { useAppStore } from '../../stores/useAppStore';

export const DiagnosticsCard: React.FC = () => {
  const { showToast, language } = useAppStore();
  const ui = (zh: string, en: string) => (language === 'en-US' ? en : zh);

  const [loading, setLoading] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [sourcesExpanded, setSourcesExpanded] = useState(false);
  const [diagnostics, setDiagnostics] = useState<DiagnosticsInfo | null>(null);

  const fetchDiagnostics = async () => {
    setLoading(true);
    try {
      const data = await tauriApi.getDiagnosticsInfo();
      setDiagnostics(data);
    } catch (err: any) {
      showToast(ui('获取系统诊断信息失败: ', 'Failed to read diagnostics: ') + (err?.message || err), 'error');
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
      showToast(ui(`诊断包已成功导出 (已脱敏): ${zipPath}`, `Sanitized diagnostics bundle exported: ${zipPath}`), 'success');
    } catch (err: any) {
      showToast(ui('导出诊断包失败: ', 'Failed to export diagnostics bundle: ') + (err?.message || err), 'error');
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
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 mb-4">
        <div className="flex items-center gap-3">
          <div className="p-2 bg-blue-50 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400 rounded-lg">
            <Activity className="w-5 h-5" />
          </div>
          <div>
            <h2 className="text-base font-semibold text-slate-800 dark:text-slate-100 flex items-center gap-2">
              {ui('系统与数据库健康诊断', 'System & database diagnostics')}
              <span className="text-xs px-2 py-0.5 rounded-full font-normal bg-blue-100 dark:bg-blue-950 text-blue-700 dark:text-blue-300">
                {ui('健康监测', 'Health')}
              </span>
            </h2>
            <p className="text-xs text-slate-500 dark:text-slate-400">
              {ui(
                '实时监控底层 SQLite/SQLCipher 完整性、FTS 全文索引状态、系统硬件及各浏览器数据源采集状况',
                'Monitors SQLite/SQLCipher integrity, FTS5 status, platform runtime, and browser source sync health'
              )}
            </p>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={fetchDiagnostics}
            disabled={loading}
            className="flex items-center gap-1.5 px-3 py-1.5 text-xs text-slate-600 dark:text-slate-300 hover:bg-slate-100 dark:hover:bg-slate-700 rounded-lg border border-slate-200 dark:border-slate-600 transition-colors cursor-pointer"
            title={ui('刷新诊断指标', 'Refresh diagnostics')}
          >
            <RefreshCw className={`w-3.5 h-3.5 ${loading ? 'animate-spin' : ''}`} />
            {ui('刷新状态', 'Refresh')}
          </button>
          <button
            type="button"
            onClick={handleExportBundle}
            disabled={exporting || !diagnostics}
            className="flex items-center gap-1.5 px-3.5 py-1.5 text-xs bg-blue-600 hover:bg-blue-700 text-white rounded-lg font-medium shadow-sm transition-colors cursor-pointer disabled:opacity-50"
          >
            <Download className="w-3.5 h-3.5" />
            {exporting ? ui('正在打包脱敏中...', 'Exporting...') : ui('导出脱敏诊断包 (.zip)', 'Export diagnostics (.zip)')}
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
                  <Database className="w-3.5 h-3.5 text-blue-500" /> {ui('数据库完整性', 'Database integrity')}
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
              <div className="text-[11px] text-slate-400 mt-0.5">
                Schema v{diagnostics.db_schema_version} · {formatBytes(diagnostics.db_size_bytes)}
                {diagnostics.sqlcipher_version
                  ? ` · SQLCipher ${diagnostics.sqlcipher_version}`
                  : diagnostics.sqlite_version
                  ? ` · SQLite ${diagnostics.sqlite_version}`
                  : ''}
              </div>
            </div>

            <div className="p-3 bg-slate-50 dark:bg-slate-900/50 rounded-lg border border-slate-200/80 dark:border-slate-700/80">
              <div className="flex items-center justify-between text-xs text-slate-500 dark:text-slate-400 mb-1">
                <span className="flex items-center gap-1">
                  <HardDrive className="w-3.5 h-3.5 text-blue-500" /> {ui('索引记录规模', 'Archive volume')}
                </span>
                <CheckCircle2 className="w-4 h-4 text-emerald-500" />
              </div>
              <div className="text-sm font-semibold text-slate-800 dark:text-slate-200">
                {ui(`${diagnostics.total_visits.toLocaleString()} 次访问`, `${diagnostics.total_visits.toLocaleString()} visits`)}
              </div>
              <div className="text-[11px] text-slate-400 mt-0.5">
                {ui(`覆盖 ${diagnostics.total_urls.toLocaleString()} 个独立 URL`, `${diagnostics.total_urls.toLocaleString()} unique URLs`)}
              </div>
            </div>

            <div className="p-3 bg-slate-50 dark:bg-slate-900/50 rounded-lg border border-slate-200/80 dark:border-slate-700/80">
              <div className="flex items-center justify-between text-xs text-slate-500 dark:text-slate-400 mb-1">
                <span className="flex items-center gap-1">
                  <Server className="w-3.5 h-3.5 text-blue-500" /> {ui('FTS5 检索索引', 'FTS5 search index')}
                </span>
                {diagnostics.fts_status === 'ok' ? (
                  <CheckCircle2 className="w-4 h-4 text-emerald-500" />
                ) : (
                  <AlertTriangle className="w-4 h-4 text-amber-500" />
                )}
              </div>
              <div className="text-sm font-semibold text-slate-800 dark:text-slate-200">
                {diagnostics.fts_status === 'ok' ? 'FTS5 Online' : ui('FTS 异常/需重建', 'FTS rebuild needed')}
              </div>
              <div className="text-[11px] text-slate-400 mt-0.5">
                {ui('全文索引就绪', 'Full-text matching ready')}
              </div>
            </div>

            <div className="p-3 bg-slate-50 dark:bg-slate-900/50 rounded-lg border border-slate-200/80 dark:border-slate-700/80">
              <div className="flex items-center justify-between text-xs text-slate-500 dark:text-slate-400 mb-1">
                <span className="flex items-center gap-1">
                  <Cpu className="w-3.5 h-3.5 text-emerald-500" /> {ui('运行平台环境', 'Runtime environment')}
                </span>
              </div>
              <div className="text-sm font-semibold text-slate-800 dark:text-slate-200 truncate">
                {diagnostics.os_name} {diagnostics.os_version}
              </div>
              <div className="text-[11px] text-slate-400 mt-0.5">
                {diagnostics.arch} · {ui(`内存 ${diagnostics.total_memory_mb} MB`, `RAM ${diagnostics.total_memory_mb} MB`)}
              </div>
            </div>
          </div>

          {/* Sources Summary Table (Collapsible) */}
          <div className="mt-3 border border-slate-200 dark:border-slate-700 rounded-lg overflow-hidden">
            <button
              type="button"
              onClick={() => setSourcesExpanded((prev) => !prev)}
              aria-expanded={sourcesExpanded}
              className="w-full px-3.5 py-2.5 bg-slate-50/80 dark:bg-slate-900/60 hover:bg-slate-100/80 dark:hover:bg-slate-800/60 text-xs font-medium text-slate-700 dark:text-slate-200 flex items-center justify-between transition-colors cursor-pointer"
            >
              <span>
                {ui(
                  `已注册浏览器数据源 (${diagnostics.sources.length} 个，已启用 ${diagnostics.sources.filter((s) => s.enabled).length} 个)`,
                  `Registered browser sources (${diagnostics.sources.length}, ${diagnostics.sources.filter((s) => s.enabled).length} enabled)`
                )}
              </span>
              <span className="text-slate-400">
                {sourcesExpanded ? <ChevronUp className="w-4 h-4" /> : <ChevronDown className="w-4 h-4" />}
              </span>
            </button>

            {sourcesExpanded && (
            <div className="max-h-40 overflow-y-auto border-t border-slate-200 dark:border-slate-700">
              <table className="w-full text-xs text-left">
                <thead className="bg-slate-100 dark:bg-slate-900/80 text-slate-500 dark:text-slate-400 font-medium">
                  <tr>
                    <th className="px-3 py-2">{ui('浏览器', 'Browser')}</th>
                    <th className="px-3 py-2">{ui('配置文件 (Profile)', 'Profile')}</th>
                    <th className="px-3 py-2">{ui('状态', 'Status')}</th>
                    <th className="px-3 py-2">{ui('最近同步记录', 'Latest activity')}</th>
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
                          className={`inline-flex items-center px-2 py-0.5 rounded-full text-[11px] font-medium ${
                            s.enabled
                              ? 'bg-emerald-100 text-emerald-800 dark:bg-emerald-950 dark:text-emerald-300'
                              : 'bg-slate-100 text-slate-600 dark:bg-slate-800 dark:text-slate-400'
                          }`}
                        >
                          {s.enabled ? ui('已启用监控', 'Enabled') : ui('已暂停', 'Paused')}
                        </span>
                      </td>
                      <td className="px-3 py-1.5 text-slate-500 dark:text-slate-400">
                        {s.last_visit_time > 0
                          ? new Date(s.last_visit_time).toLocaleString()
                          : ui('尚未同步', 'Not synced yet')}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            )}
          </div>

          {/* Privacy Disclaimer Banner */}
          <div className="flex items-start gap-2.5 p-3 bg-emerald-50 dark:bg-emerald-950/30 border border-emerald-200 dark:border-emerald-800/50 rounded-lg text-xs text-emerald-800 dark:text-emerald-300">
            <ShieldCheck className="w-4 h-4 flex-shrink-0 mt-0.5 text-emerald-600 dark:text-emerald-400" />
            <div>
              <p className="font-medium">{ui('零浏览历史诊断导出承诺', 'Zero-history diagnostics guarantee')}</p>
              <p className="text-xs opacity-90 mt-0.5">
                {ui(
                  '诊断包仅提取系统配置、数据库统计指标与脱敏运行日志，不包含任何真实浏览历史、URL、网页标题、搜索词、API Key 或 PIN 密钥。',
                  'Exports only environment metadata, database counts, and redacted runtime logs—never browsing history, URLs, titles, search terms, API keys, or PINs.'
                )}
              </p>
            </div>
          </div>
        </div>
      ) : (
        <div className="py-8 text-center text-xs text-slate-400">
          {ui('正在读取系统诊断指标...', 'Loading system diagnostics...')}
        </div>
      )}
    </div>
  );
};
