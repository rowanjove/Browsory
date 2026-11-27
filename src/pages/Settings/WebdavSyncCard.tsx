import React, { useState, useEffect } from 'react';
import {
  Cloud,
  Lock,
  RefreshCw,
  CheckCircle2,
  AlertCircle,
  FolderSync,
  ShieldCheck,
  Server,
  ChevronDown,
  ChevronUp,
} from 'lucide-react';
import { tauriApi } from '../../services/tauri';
import { WebDavConfig, SyncStatusReport } from '../../types';
import { useAppStore } from '../../stores/useAppStore';

export const WebdavSyncCard: React.FC = () => {
  const { language } = useAppStore();
  const ui = (zh: string, en: string) => (language === 'en-US' ? en : zh);

  const [config, setConfig] = useState<WebDavConfig>({
    enabled: false,
    server_url: '',
    username: '',
    password: '',
    remote_dir: '/browsory',
  });

  const [expanded, setExpanded] = useState(false);
  const [testing, setTesting] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [saving, setSaving] = useState(false);
  const [testResult, setTestResult] = useState<{ success: boolean; msg: string } | null>(null);
  const [syncReport, setSyncReport] = useState<SyncStatusReport | null>(null);

  useEffect(() => {
    const loadSettings = async () => {
      try {
        const saved = await tauriApi.getSetting('webdav_sync_config');
        if (saved) {
          const parsed = JSON.parse(saved);
          setConfig(parsed);
        }
      } catch (err) {
        console.warn('No saved WebDAV config or parse error:', err);
      }
    };
    loadSettings();
  }, []);

  const handleSaveConfig = async () => {
    setSaving(true);
    try {
      await tauriApi.setSetting('webdav_sync_config', JSON.stringify(config));
      setTestResult({
        success: true,
        msg: ui('WebDAV 配置已保存，密码写入系统安全存储', 'WebDAV configuration saved; password stored in OS keychain'),
      });
    } catch (err: any) {
      setTestResult({ success: false, msg: `${ui('保存失败: ', 'Save failed: ')}${err?.toString()}` });
    } finally {
      setSaving(false);
    }
  };

  const handleTestConnection = async () => {
    if (!config.server_url || !config.username) {
      setTestResult({
        success: false,
        msg: ui('请先填写 WebDAV 服务器地址和用户名', 'Enter the WebDAV server URL and username first'),
      });
      return;
    }
    setTesting(true);
    setTestResult(null);
    try {
      await tauriApi.testWebDavSync(config);
      setTestResult({ success: true, msg: ui('WebDAV 服务器连接成功，鉴权通过！', 'WebDAV connection and authentication succeeded!') });
    } catch (err: any) {
      setTestResult({
        success: false,
        msg: `${ui('连接失败: ', 'Connection failed: ')}${err?.toString() || ui('无法连接服务器', 'Server unreachable')}`,
      });
    } finally {
      setTesting(false);
    }
  };

  const handleTriggerSync = async () => {
    setSyncing(true);
    setSyncReport(null);
    try {
      const report = await tauriApi.executeWebDavSync(config);
      setSyncReport(report);
    } catch (err: any) {
      setSyncReport({
        success: false,
        uploaded_items: 0,
        downloaded_items: 0,
        message: `${ui('同步中断: ', 'Sync aborted: ')}${err?.toString()}`,
        timestamp: Date.now(),
      });
    } finally {
      setSyncing(false);
    }
  };

  return (
    <div className="rounded-2xl border border-slate-200 dark:border-slate-800 bg-white dark:bg-slate-900 shadow-xs overflow-hidden">
      <div className="p-5 flex items-center justify-between gap-4">
        <button
          type="button"
          onClick={() => setExpanded((prev) => !prev)}
          aria-expanded={expanded}
          className="flex-1 flex items-center justify-between gap-3 text-left cursor-pointer min-w-0"
        >
          <div className="flex items-center gap-3 min-w-0">
            <div className="p-2.5 rounded-xl bg-blue-500/10 text-blue-600 dark:text-blue-400 border border-blue-500/20 shrink-0">
              <Cloud className="w-5 h-5" />
            </div>
            <div className="min-w-0">
              <div className="flex items-center gap-2 flex-wrap">
                <h3 className="text-sm font-semibold text-slate-900 dark:text-slate-100">
                  {ui('WebDAV 连接设置', 'WebDAV connection settings')}
                </h3>
                <span className="flex items-center gap-1 px-2 py-0.5 text-[11px] font-medium rounded-full bg-emerald-50 dark:bg-emerald-950/60 text-emerald-700 dark:text-emerald-300 border border-emerald-200 dark:border-emerald-800">
                  <ShieldCheck className="w-3 h-3" />
                  {ui('凭据安全存储', 'OS secure keychain')}
                </span>
              </div>
              <p className="text-xs text-slate-500 dark:text-slate-400 mt-0.5 truncate">
                {config.server_url.trim()
                  ? `${config.enabled ? ui('已配置端点', 'Configured') : ui('已暂停', 'Disabled')}: ${config.server_url}`
                  : ui(
                      '支持 Nextcloud、群晖 NAS、坚果云等 HTTPS 端点连接测试（当前未展开配置）',
                      'Supports HTTPS testing for Nextcloud, NAS, etc. (Click to configure)'
                    )}
              </p>
            </div>
          </div>
          <span className="p-1 text-slate-400 shrink-0">
            {expanded ? <ChevronUp className="w-4 h-4" /> : <ChevronDown className="w-4 h-4" />}
          </span>
        </button>

        <label className="relative inline-flex items-center cursor-pointer shrink-0">
          <input
            type="checkbox"
            checked={config.enabled}
            aria-label={ui('启用 WebDAV 配置', 'Enable WebDAV config')}
            onChange={(e) => setConfig({ ...config, enabled: e.target.checked })}
            className="sr-only peer"
          />
          <div className="w-9 h-5 bg-slate-200 peer-focus:outline-hidden rounded-full peer dark:bg-slate-700 peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-slate-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all dark:border-slate-600 peer-checked:bg-blue-600"></div>
        </label>
      </div>

      {expanded && (
      <div className="space-y-3 px-5 pb-5 pt-3 border-t border-slate-100 dark:border-slate-800">
        <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
          <div className="flex flex-col gap-1">
            <label className="text-xs font-medium text-slate-700 dark:text-slate-300 flex items-center gap-1.5">
              <Server className="w-3.5 h-3.5 text-slate-400" /> {ui('WebDAV 服务器地址', 'WebDAV server URL')}
            </label>
            <input
              type="text"
              value={config.server_url}
              onChange={(e) => setConfig({ ...config, server_url: e.target.value })}
              placeholder="https://dav.example.com/dav/"
              className="px-3 py-2 text-xs rounded-xl border border-slate-200 dark:border-slate-800 bg-slate-50 dark:bg-slate-950 font-mono text-slate-800 dark:text-slate-200 focus:outline-hidden focus:ring-1 focus:ring-blue-500"
            />
            <div className="flex gap-2 text-[11px] text-slate-400 mt-0.5">
              <button
                type="button"
                onClick={() => setConfig({ ...config, server_url: 'https://dav.jianguoyun.com/dav/' })}
                className="hover:text-blue-500 underline cursor-pointer"
              >
                {ui('坚果云', 'Jianguoyun')}
              </button>
              <button
                type="button"
                onClick={() => setConfig({ ...config, server_url: 'https://your-nextcloud.com/remote.php/dav/files/user/' })}
                className="hover:text-blue-500 underline cursor-pointer"
              >
                Nextcloud
              </button>
            </div>
          </div>

          <div className="flex flex-col gap-1">
            <label className="text-xs font-medium text-slate-700 dark:text-slate-300">
              {ui('远端同步存储目录', 'Remote directory')}
            </label>
            <input
              type="text"
              value={config.remote_dir}
              onChange={(e) => setConfig({ ...config, remote_dir: e.target.value })}
              placeholder="/browsory"
              className="px-3 py-2 text-xs rounded-xl border border-slate-200 dark:border-slate-800 bg-slate-50 dark:bg-slate-950 font-mono text-slate-800 dark:text-slate-200 focus:outline-hidden focus:ring-1 focus:ring-blue-500"
            />
          </div>

          <div className="flex flex-col gap-1">
            <label className="text-xs font-medium text-slate-700 dark:text-slate-300">
              {ui('WebDAV 账户名 / 邮箱', 'Username / email')}
            </label>
            <input
              type="text"
              value={config.username}
              onChange={(e) => setConfig({ ...config, username: e.target.value })}
              placeholder="user@example.com"
              className="px-3 py-2 text-xs rounded-xl border border-slate-200 dark:border-slate-800 bg-slate-50 dark:bg-slate-950 text-slate-800 dark:text-slate-200 focus:outline-hidden focus:ring-1 focus:ring-blue-500"
            />
          </div>

          <div className="flex flex-col gap-1">
            <label className="text-xs font-medium text-slate-700 dark:text-slate-300 flex items-center gap-1.5">
              <Lock className="w-3.5 h-3.5 text-slate-400" /> {ui('应用授权密码 / 访问令牌', 'App password / token')}
            </label>
            <input
              type="password"
              value={config.password}
              onChange={(e) => setConfig({ ...config, password: e.target.value })}
              placeholder={ui('请输入独立生成的应用密码', 'Enter generated app password')}
              className="px-3 py-2 text-xs rounded-xl border border-slate-200 dark:border-slate-800 bg-slate-50 dark:bg-slate-950 font-mono text-slate-800 dark:text-slate-200 focus:outline-hidden focus:ring-1 focus:ring-blue-500"
            />
            <p className="text-[11px] text-slate-500 dark:text-slate-400">
              {ui('密码按服务器地址和账号分别保存；更换服务器或账号后请重新输入。', 'Passwords are stored per server and account. Re-enter after changing either.')}
            </p>
          </div>
        </div>

        <div className="flex flex-wrap items-center gap-2 pt-2">
          <button
            type="button"
            onClick={handleSaveConfig}
            disabled={saving}
            className="px-3 py-1.5 text-xs font-medium rounded-xl border border-slate-200 dark:border-slate-800 hover:bg-slate-100 dark:hover:bg-slate-800 text-slate-700 dark:text-slate-200 transition cursor-pointer"
          >
            {saving ? ui('正在保存...', 'Saving...') : ui('保存同步设置', 'Save WebDAV config')}
          </button>

          <button
            type="button"
            onClick={handleTestConnection}
            disabled={testing}
            className="px-3 py-1.5 text-xs font-medium rounded-xl border border-blue-200 dark:border-blue-900/60 bg-blue-50/50 dark:bg-blue-950/40 text-blue-600 dark:text-blue-400 hover:bg-blue-100 dark:hover:bg-blue-900/60 transition cursor-pointer flex items-center gap-1.5"
          >
            <RefreshCw className={`w-3 h-3 ${testing ? 'animate-spin' : ''}`} />
            <span>{testing ? ui('连接中...', 'Testing...') : ui('测试服务器联通性', 'Test connection')}</span>
          </button>

          <button
            type="button"
            onClick={handleTriggerSync}
            disabled
            className="px-4 py-1.5 text-xs font-medium rounded-xl bg-blue-600 disabled:opacity-50 text-white transition flex items-center gap-1.5 shadow-xs ml-auto"
          >
            <FolderSync className={`w-3.5 h-3.5 ${syncing ? 'animate-spin' : ''}`} />
            <span>{ui('双向同步暂不可用', 'Two-way sync unavailable')}</span>
          </button>
        </div>

        {testResult && (
          <div
            role="status"
            className={`flex items-center gap-2 p-2.5 rounded-xl text-xs border ${
              testResult.success
                ? 'bg-emerald-50 dark:bg-emerald-950/40 text-emerald-600 dark:text-emerald-400 border-emerald-200 dark:border-emerald-900/40'
                : 'bg-red-50 dark:bg-red-950/40 text-red-600 dark:text-red-400 border-red-200 dark:border-red-900/40'
            }`}
          >
            {testResult.success ? <CheckCircle2 className="w-4 h-4 shrink-0" /> : <AlertCircle className="w-4 h-4 shrink-0" />}
            <span>{testResult.msg}</span>
          </div>
        )}

        {syncReport && (
          <div
            role="status"
            className={`flex items-center gap-2 p-2.5 rounded-xl text-xs border ${
              syncReport.success
                ? 'bg-emerald-50 dark:bg-emerald-950/40 text-emerald-600 dark:text-emerald-400 border-emerald-200 dark:border-emerald-900/40'
                : 'bg-red-50 dark:bg-red-950/40 text-red-600 dark:text-red-400 border-red-200 dark:border-red-900/40'
            }`}
          >
            {syncReport.success ? <CheckCircle2 className="w-4 h-4 shrink-0" /> : <AlertCircle className="w-4 h-4 shrink-0" />}
            <span>{syncReport.message}</span>
          </div>
        )}
      </div>
      )}
    </div>
  );
};
