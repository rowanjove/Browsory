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
} from 'lucide-react';
import { tauriApi } from '../../services/tauri';
import { WebDavConfig, SyncStatusReport } from '../../types';

export const WebdavSyncCard: React.FC = () => {
  const [config, setConfig] = useState<WebDavConfig>({
    enabled: false,
    server_url: '',
    username: '',
    password: '',
    remote_dir: '/browsory',
  });

  const [testing, setTesting] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [saving, setSaving] = useState(false);
  const [testResult, setTestResult] = useState<{ success: boolean; msg: string } | null>(null);
  const [syncReport, setSyncReport] = useState<SyncStatusReport | null>(null);

  useEffect(() => {
    // 读取保存的 WebDAV 配置
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
      setTestResult({ success: true, msg: 'WebDAV 配置已保存，密码写入系统安全存储' });
    } catch (err: any) {
      setTestResult({ success: false, msg: `保存失败: ${err?.toString()}` });
    } finally {
      setSaving(false);
    }
  };

  const handleTestConnection = async () => {
    if (!config.server_url || !config.username) {
      setTestResult({ success: false, msg: '请先填写 WebDAV 服务器地址和用户名' });
      return;
    }
    setTesting(true);
    setTestResult(null);
    try {
      await tauriApi.testWebDavSync(config);
      setTestResult({ success: true, msg: 'WebDAV 服务器连接成功，鉴权通过！' });
    } catch (err: any) {
      setTestResult({
        success: false,
        msg: `连接失败: ${err?.toString() || '无法连接服务器'}`,
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
        message: `同步中断: ${err?.toString()}`,
        timestamp: Date.now(),
      });
    } finally {
      setSyncing(false);
    }
  };

  return (
    <div className="p-5 rounded-2xl border border-slate-200 dark:border-slate-800 bg-white dark:bg-slate-900 shadow-xs space-y-4">
      {/* Header */}
      <div className="flex items-start justify-between gap-4">
        <div className="flex items-center gap-3">
          <div className="p-2.5 rounded-xl bg-blue-500/10 text-blue-600 dark:text-blue-400 border border-blue-500/20">
            <Cloud className="w-5 h-5" />
          </div>
          <div>
            <div className="flex items-center gap-2">
              <h3 className="text-sm font-semibold text-slate-900 dark:text-slate-100">
                WebDAV 加密传输设置
              </h3>
              <span className="flex items-center gap-1 px-2 py-0.5 text-[10px] font-medium rounded-full bg-emerald-50 dark:bg-emerald-950/60 text-emerald-700 dark:text-emerald-300 border border-emerald-200 dark:border-emerald-800">
                <ShieldCheck className="w-3 h-3" />
                凭据安全存储
              </span>
            </div>
            <p className="text-xs text-slate-500 dark:text-slate-400 mt-0.5">
              支持连接坚果云、Nextcloud、群晖 NAS 等私有网盘。当前版本仅提供 HTTPS 连接测试；双向下载、冲突合并和上传尚未开放。
            </p>
          </div>
        </div>

        <label className="relative inline-flex items-center cursor-pointer">
          <input
            type="checkbox"
            checked={config.enabled}
            onChange={(e) => setConfig({ ...config, enabled: e.target.checked })}
            className="sr-only peer"
          />
          <div className="w-9 h-5 bg-slate-200 peer-focus:outline-hidden rounded-full peer dark:bg-slate-700 peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-slate-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all dark:border-slate-600 peer-checked:bg-blue-600"></div>
        </label>
      </div>

      {/* Form Fields */}
      <div className="space-y-3 pt-2">
        <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
          <div className="flex flex-col gap-1">
            <label className="text-xs font-medium text-slate-700 dark:text-slate-300 flex items-center gap-1.5">
              <Server className="w-3.5 h-3.5 text-slate-400" /> WebDAV 服务器地址
            </label>
            <input
              type="text"
              value={config.server_url}
              onChange={(e) => setConfig({ ...config, server_url: e.target.value })}
              placeholder="https://dav.jianguoyun.com/dav/"
              className="px-3 py-2 text-xs rounded-xl border border-slate-200 dark:border-slate-800 bg-slate-50 dark:bg-slate-950 font-mono text-slate-800 dark:text-slate-200 focus:outline-hidden focus:ring-1 focus:ring-blue-500"
            />
            <div className="flex gap-2 text-[10px] text-slate-400 mt-0.5">
              <button
                type="button"
                onClick={() =>
                  setConfig({ ...config, server_url: 'https://dav.jianguoyun.com/dav/' })
                }
                className="hover:text-blue-500 underline cursor-pointer"
              >
                坚果云
              </button>
              <button
                type="button"
                onClick={() =>
                  setConfig({ ...config, server_url: 'https://your-nextcloud.com/remote.php/dav/files/user/' })
                }
                className="hover:text-blue-500 underline cursor-pointer"
              >
                Nextcloud
              </button>
            </div>
          </div>

          <div className="flex flex-col gap-1">
            <label className="text-xs font-medium text-slate-700 dark:text-slate-300">
              远端同步存储目录
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
              WebDAV 账户名 / 邮箱
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
              <Lock className="w-3.5 h-3.5 text-slate-400" /> 应用授权密码 / 访问令牌
            </label>
            <input
              type="password"
              value={config.password}
              onChange={(e) => setConfig({ ...config, password: e.target.value })}
              placeholder="请输入独立生成的应用密码"
              className="px-3 py-2 text-xs rounded-xl border border-slate-200 dark:border-slate-800 bg-slate-50 dark:bg-slate-950 font-mono text-slate-800 dark:text-slate-200 focus:outline-hidden focus:ring-1 focus:ring-blue-500"
            />
          </div>
        </div>

        {/* Action Buttons */}
        <div className="flex items-center gap-2 pt-2">
          <button
            onClick={handleSaveConfig}
            disabled={saving}
            className="px-3 py-1.5 text-xs font-medium rounded-xl border border-slate-200 dark:border-slate-800 hover:bg-slate-100 dark:hover:bg-slate-800 text-slate-700 dark:text-slate-200 transition cursor-pointer"
          >
            {saving ? '正在保存...' : '保存同步设置'}
          </button>

          <button
            onClick={handleTestConnection}
            disabled={testing}
            className="px-3 py-1.5 text-xs font-medium rounded-xl border border-blue-200 dark:border-blue-900/60 bg-blue-50/50 dark:bg-blue-950/40 text-blue-600 dark:text-blue-400 hover:bg-blue-100 dark:hover:bg-blue-900/60 transition cursor-pointer flex items-center gap-1.5"
          >
            <RefreshCw className={`w-3 h-3 ${testing ? 'animate-spin' : ''}`} />
            <span>{testing ? '连接中...' : '测试服务器联通性'}</span>
          </button>

          <button
            onClick={handleTriggerSync}
            disabled
            className="px-4 py-1.5 text-xs font-medium rounded-xl bg-blue-600 hover:bg-blue-700 disabled:opacity-50 text-white transition flex items-center gap-1.5 cursor-pointer shadow-xs ml-auto"
          >
            <FolderSync className={`w-3.5 h-3.5 ${syncing ? 'animate-spin' : ''}`} />
            <span>双向同步暂不可用</span>
          </button>
        </div>

        {/* Feedback Alerts */}
        {testResult && (
          <div
            className={`flex items-center gap-2 p-2.5 rounded-xl text-xs border ${
              testResult.success
                ? 'bg-emerald-50 dark:bg-emerald-950/40 text-emerald-600 dark:text-emerald-400 border-emerald-200 dark:border-emerald-900/40'
                : 'bg-red-50 dark:bg-red-950/40 text-red-600 dark:text-red-400 border-red-200 dark:border-red-900/40'
            }`}
          >
            {testResult.success ? (
              <CheckCircle2 className="w-4 h-4 shrink-0" />
            ) : (
              <AlertCircle className="w-4 h-4 shrink-0" />
            )}
            <span>{testResult.msg}</span>
          </div>
        )}

        {syncReport && (
          <div
            className={`flex items-center gap-2 p-2.5 rounded-xl text-xs border ${
              syncReport.success
                ? 'bg-emerald-50 dark:bg-emerald-950/40 text-emerald-600 dark:text-emerald-400 border-emerald-200 dark:border-emerald-900/40'
                : 'bg-red-50 dark:bg-red-950/40 text-red-600 dark:text-red-400 border-red-200 dark:border-red-900/40'
            }`}
          >
            {syncReport.success ? (
              <CheckCircle2 className="w-4 h-4 shrink-0" />
            ) : (
              <AlertCircle className="w-4 h-4 shrink-0" />
            )}
            <span>{syncReport.message}</span>
          </div>
        )}
      </div>
    </div>
  );
};
