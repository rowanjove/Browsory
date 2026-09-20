import React, { useState, useEffect } from 'react';
import {
  Crown,
  ShieldCheck,
  Key,
  CheckCircle2,
  AlertCircle,
  Sparkles,
  RefreshCw,
  Trash2,
  Zap,
} from 'lucide-react';
import { useLicenseStore } from '../../stores/useLicenseStore';

export const LicenseSettingsCard: React.FC = () => {
  const { license, loading, error, fetchLicense, activate, deactivate } = useLicenseStore();
  const [inputKey, setInputKey] = useState('');
  const [successMsg, setSuccessMsg] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    fetchLicense();
  }, [fetchLicense]);

  const handleActivate = async () => {
    if (!inputKey.trim()) return;
    setSubmitting(true);
    setSuccessMsg(null);
    const ok = await activate(inputKey.trim());
    setSubmitting(false);
    if (ok) {
      setSuccessMsg('Browsory Pro 授权激活成功！已开启所有专业功能');
      setInputKey('');
    }
  };

  const handleDeactivate = async () => {
    if (window.confirm('确定要移除当前 Pro 授权并回退至开源免费版吗？')) {
      await deactivate();
      setSuccessMsg(null);
    }
  };

  const isPro = license?.is_pro ?? false;

  return (
    <div className="p-5 rounded-2xl border border-slate-200 dark:border-slate-800 bg-white dark:bg-slate-900 shadow-xs space-y-4">
      {/* Header */}
      <div className="flex items-start justify-between gap-4">
        <div className="flex items-center gap-3">
          <div
            className={`p-2.5 rounded-xl ${
              isPro
                ? 'bg-amber-500/10 text-amber-600 dark:text-amber-400 border border-amber-500/20'
                : 'bg-blue-500/10 text-blue-600 dark:text-blue-400 border border-blue-500/20'
            }`}
          >
            {isPro ? <Crown className="w-5 h-5" /> : <ShieldCheck className="w-5 h-5" />}
          </div>
          <div>
            <div className="flex items-center gap-2">
              <h3 className="text-sm font-semibold text-slate-900 dark:text-slate-100">
                软件授权与版本管理
              </h3>
              <span
                className={`px-2 py-0.5 text-xs font-medium rounded-full ${
                  isPro
                    ? 'bg-amber-100 dark:bg-amber-950/60 text-amber-700 dark:text-amber-300 border border-amber-300 dark:border-amber-800'
                    : 'bg-slate-100 dark:bg-slate-800 text-slate-600 dark:text-slate-400'
                }`}
              >
                {isPro ? 'Browsory Pro 专业版' : 'Browsory 免费版'}
              </span>
            </div>
            <p className="text-xs text-slate-500 dark:text-slate-400 mt-0.5">
              本地优先存储，免费版不限历史条数；Pro 版提供进阶本地智能与端到端安全
            </p>
          </div>
        </div>

        <button
          onClick={() => fetchLicense()}
          disabled={loading}
          className="p-1.5 rounded-lg text-slate-400 hover:text-slate-600 dark:hover:text-slate-200 hover:bg-slate-100 dark:hover:bg-slate-800 transition cursor-pointer"
          title="刷新授权状态"
        >
          <RefreshCw className={`w-4 h-4 ${loading ? 'animate-spin' : ''}`} />
        </button>
      </div>

      {/* Pro Status Details or Activation Box */}
      {isPro ? (
        <div className="p-4 rounded-xl border border-amber-200/80 dark:border-amber-900/60 bg-gradient-to-br from-amber-50/40 to-orange-50/20 dark:from-amber-950/20 dark:to-orange-950/10 space-y-3">
          <div className="flex items-center justify-between text-xs">
            <span className="text-slate-500 dark:text-slate-400">授权用户:</span>
            <span className="font-medium text-slate-800 dark:text-slate-200">
              {license?.licensee_name || 'Browsory Pro 用户'}
            </span>
          </div>

          <div className="flex items-center justify-between text-xs">
            <span className="text-slate-500 dark:text-slate-400">授权方案:</span>
            <span className="font-mono font-medium text-amber-700 dark:text-amber-300 uppercase">
              {license?.plan === 'lifetime' ? '永久授权 (Lifetime)' : 'Pro 商业授权'}
            </span>
          </div>

          <div className="flex items-center justify-between text-xs">
            <span className="text-slate-500 dark:text-slate-400">设备绑定上限:</span>
            <span className="font-medium text-slate-800 dark:text-slate-200">
              {license?.device_limit ?? 3} 台设备
            </span>
          </div>

          {/* Pro Benefits */}
          <div className="pt-2 border-t border-amber-200/60 dark:border-amber-900/40 space-y-1.5">
            <span className="text-[11px] font-semibold text-amber-800 dark:text-amber-300 flex items-center gap-1">
              <Zap className="w-3.5 h-3.5" /> 已激活的 Pro 特权：
            </span>
            <div className="grid grid-cols-2 gap-1.5 text-xs text-slate-600 dark:text-slate-300">
              <span className="flex items-center gap-1">
                <CheckCircle2 className="w-3.5 h-3.5 text-emerald-500" /> 本地 PIN 保护与安全备份
              </span>
              <span className="flex items-center gap-1">
                <CheckCircle2 className="w-3.5 h-3.5 text-emerald-500" /> 4 层语义混合检索与召回
              </span>
              <span className="flex items-center gap-1">
                <CheckCircle2 className="w-3.5 h-3.5 text-emerald-500" /> 离线阅读索引与沙箱渲染
              </span>
              <span className="flex items-center gap-1">
                <CheckCircle2 className="w-3.5 h-3.5 text-emerald-500" /> WebDAV HTTPS 凭据保护
              </span>
            </div>
          </div>

          <div className="pt-2 flex justify-end">
            <button
              onClick={handleDeactivate}
              className="flex items-center gap-1 text-xs text-red-600 dark:text-red-400 hover:underline cursor-pointer"
            >
              <Trash2 className="w-3 h-3" />
              <span>移除本地授权证书</span>
            </button>
          </div>
        </div>
      ) : (
        <div className="space-y-3 pt-1">
          <div className="flex flex-col gap-1.5">
            <label className="text-xs font-medium text-slate-700 dark:text-slate-300 flex items-center gap-1.5">
              <Key className="w-3.5 h-3.5 text-slate-400" /> 输入 Pro 授权码 (License Key)
            </label>
            <div className="flex gap-2">
              <input
                type="text"
                value={inputKey}
                onChange={(e) => setInputKey(e.target.value)}
                placeholder="粘贴您的官方授权码 (格式: Payload.Signature)"
                className="flex-1 px-3 py-2 text-xs rounded-xl border border-slate-200 dark:border-slate-800 bg-slate-50 dark:bg-slate-950 font-mono text-slate-800 dark:text-slate-200 focus:outline-hidden focus:ring-1 focus:ring-blue-500"
              />
              <button
                onClick={handleActivate}
                disabled={submitting || !inputKey.trim()}
                className="px-4 py-2 text-xs font-medium rounded-xl bg-blue-600 hover:bg-blue-700 disabled:opacity-50 text-white transition flex items-center gap-1.5 cursor-pointer shadow-xs"
              >
                <Sparkles className="w-3.5 h-3.5" />
                <span>{submitting ? '校验中...' : '立即激活'}</span>
              </button>
            </div>
          </div>

          {error && (
            <div className="flex items-center gap-2 p-2.5 rounded-xl bg-red-50 dark:bg-red-950/40 text-red-600 dark:text-red-400 text-xs border border-red-200 dark:border-red-900/40">
              <AlertCircle className="w-4 h-4 shrink-0" />
              <span>{error}</span>
            </div>
          )}

          {successMsg && (
            <div className="flex items-center gap-2 p-2.5 rounded-xl bg-emerald-50 dark:bg-emerald-950/40 text-emerald-600 dark:text-emerald-400 text-xs border border-emerald-200 dark:border-emerald-900/40">
              <CheckCircle2 className="w-4 h-4 shrink-0" />
              <span>{successMsg}</span>
            </div>
          )}
        </div>
      )}
    </div>
  );
};
