import React, { useState, useEffect } from 'react';
import {
  ShieldCheck,
  Compass,
  Database,
  Lock,
  ArrowRight,
  CheckCircle2,
  RefreshCw,
  Copy,
  Check,
  Globe,
  Sparkles,
  Search,
} from 'lucide-react';
import { tauriApi } from '../../services/tauri';
import { useSecurityStore } from '../../stores/useSecurityStore';
import { useAppStore } from '../../stores/useAppStore';
import { DiscoveredProfile } from '../../types';
import { AppLogo } from '../Common/AppLogo';

interface OnboardingWizardProps {
  isOpen: boolean;
  onComplete: () => void;
}

type Step = 'welcome' | 'discovery' | 'security' | 'ready';

export const OnboardingWizard: React.FC<OnboardingWizardProps> = ({
  isOpen,
  onComplete,
}) => {
  const { showToast } = useAppStore();
  const { setOrChangePin, triggerSyncAll } = useSecurityStore();

  const [currentStep, setCurrentStep] = useState<Step>('welcome');
  const [profiles, setProfiles] = useState<DiscoveredProfile[]>([]);
  const [isScanning, setIsScanning] = useState(false);
  const [isSyncing, setIsSyncing] = useState(false);
  const [syncedCount, setSyncedCount] = useState<number | null>(null);

  // PIN settings state
  const [pin, setPin] = useState('');
  const [confirmPin, setConfirmPin] = useState('');
  const [pinError, setPinError] = useState('');
  const [recoveryKey, setRecoveryKey] = useState<string | null>(null);
  const [copiedKey, setCopiedKey] = useState(false);
  const [settingPin, setSettingPin] = useState(false);

  // Load browser discovery when reaching discovery step
  useEffect(() => {
    if (currentStep === 'discovery') {
      setIsScanning(true);
      tauriApi
        .scanBrowsers()
        .then((res) => {
          setProfiles(res);
        })
        .catch((err) => {
          console.error('Failed to scan browsers:', err);
        })
        .finally(() => {
          setIsScanning(false);
        });
    }
  }, [currentStep]);

  if (!isOpen) return null;

  const handleStartInitialSync = async () => {
    setIsSyncing(true);
    try {
      const count = await triggerSyncAll();
      setSyncedCount(count);
      showToast(`首次同步完成，成功导入 ${count} 条访问足迹`, 'success');
    } catch (err) {
      showToast(typeof err === 'string' ? err : '同步出错', 'error');
    } finally {
      setIsSyncing(false);
    }
  };

  const handleSavePin = async (e: React.FormEvent) => {
    e.preventDefault();
    setPinError('');

    if (pin.length < 4 || pin.length > 6 || !/^\d+$/.test(pin)) {
      setPinError('PIN 码必须为 4 至 6 位纯数字');
      return;
    }

    if (pin !== confirmPin) {
      setPinError('两次输入的 PIN 码不一致');
      return;
    }

    setSettingPin(true);
    try {
      const key = await setOrChangePin(null, pin);
      setRecoveryKey(key);
      showToast('应用安全 PIN 码设置成功', 'success');
    } catch (err) {
      setPinError(typeof err === 'string' ? err : '设置 PIN 码失败');
    } finally {
      setSettingPin(false);
    }
  };

  const handleCopyRecoveryKey = () => {
    if (!recoveryKey) return;
    navigator.clipboard.writeText(recoveryKey).then(() => {
      setCopiedKey(true);
      setTimeout(() => setCopiedKey(false), 2000);
    });
  };

  const finishOnboarding = () => {
    localStorage.setItem('browsory_onboarded_v1', 'true');
    onComplete();
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/75 backdrop-blur-md animate-in fade-in duration-300 select-none">
      <div className="w-full max-w-2xl bg-white dark:bg-slate-900 border border-slate-200 dark:border-slate-800 rounded-3xl shadow-2xl overflow-hidden flex flex-col max-h-[90vh]">
        {/* Step Indicator Top Bar */}
        <div className="px-8 pt-6 pb-4 border-b border-slate-100 dark:border-slate-800 flex items-center justify-between">
          <div className="flex items-center gap-2.5">
            <AppLogo size={24} />
            <span className="font-bold text-sm tracking-tight text-slate-900 dark:text-slate-100">
              Browsory
            </span>
          </div>

          <div className="flex items-center gap-2">
            {(['welcome', 'discovery', 'security', 'ready'] as Step[]).map((step, idx) => {
              const stepIdx = ['welcome', 'discovery', 'security', 'ready'].indexOf(currentStep);
              const isActive = idx === stepIdx;
              const isPast = idx < stepIdx;
              return (
                <div key={step} className="flex items-center gap-2">
                  <div
                    className={`w-2.5 h-2.5 rounded-full transition-all ${
                      isActive
                        ? 'bg-blue-600 w-6'
                        : isPast
                        ? 'bg-blue-400 dark:bg-blue-500'
                        : 'bg-slate-200 dark:bg-slate-700'
                    }`}
                  />
                </div>
              );
            })}
          </div>
        </div>

        {/* Modal Body */}
        <div className="p-8 overflow-y-auto flex-1">
          {/* Step 1: Welcome & Privacy */}
          {currentStep === 'welcome' && (
            <div className="space-y-6 animate-in fade-in duration-200">
              <div className="space-y-2">
                <span className="inline-flex items-center gap-1.5 px-3 py-1 rounded-full text-xs font-semibold bg-blue-50 dark:bg-blue-950 text-blue-600 dark:text-blue-400 border border-blue-200 dark:border-blue-800">
                  <Sparkles className="w-3.5 h-3.5" />
                  欢迎使用 Browsory
                </span>
                <h2 className="text-2xl font-black text-slate-900 dark:text-slate-100 tracking-tight">
                  你的私人本地网络记忆库
                </h2>
                <p className="text-sm text-slate-500 dark:text-slate-400 leading-relaxed">
                  Browsory 帮助你自动汇聚散落在各个浏览器的历史足迹，打破设备与平台隔阂，并提供深度检索、智能重聚与记忆重温能力。
                </p>
              </div>

              {/* 3 Core Principles */}
              <div className="space-y-3">
                <div className="p-4 rounded-2xl bg-slate-50 dark:bg-slate-800/60 border border-slate-200/80 dark:border-slate-800 flex items-start gap-3.5">
                  <div className="p-2 rounded-xl bg-blue-100 dark:bg-blue-950/80 text-blue-600 dark:text-blue-400 shrink-0">
                    <ShieldCheck className="w-5 h-5" />
                  </div>
                  <div className="space-y-0.5">
                    <h4 className="text-sm font-bold text-slate-900 dark:text-slate-100">
                      100% 本地优先 (Local-First)
                    </h4>
                    <p className="text-xs text-slate-500 dark:text-slate-400 leading-normal">
                      所有浏览历史保存在本机独立 SQLite 数据库中。无需强制注册云端账号，无后台隐秘追踪。
                    </p>
                  </div>
                </div>

                <div className="p-4 rounded-2xl bg-slate-50 dark:bg-slate-800/60 border border-slate-200/80 dark:border-slate-800 flex items-start gap-3.5">
                  <div className="p-2 rounded-xl bg-emerald-100 dark:bg-emerald-950/80 text-emerald-600 dark:text-emerald-400 shrink-0">
                    <Database className="w-5 h-5" />
                  </div>
                  <div className="space-y-0.5">
                    <h4 className="text-sm font-bold text-slate-900 dark:text-slate-100">
                      数据永久属于你，无条数阉割
                    </h4>
                    <p className="text-xs text-slate-500 dark:text-slate-400 leading-normal">
                      不同于普通浏览器仅保留 90 天，Browsory 永久留存你的每一次探索，不设置任何数据量或天数门槛。
                    </p>
                  </div>
                </div>

                <div className="p-4 rounded-2xl bg-slate-50 dark:bg-slate-800/60 border border-slate-200/80 dark:border-slate-800 flex items-start gap-3.5">
                  <div className="p-2 rounded-xl bg-purple-100 dark:bg-purple-950/80 text-purple-600 dark:text-purple-400 shrink-0">
                    <Sparkles className="w-5 h-5" />
                  </div>
                  <div className="space-y-0.5">
                    <h4 className="text-sm font-bold text-slate-900 dark:text-slate-100">
                      语义网络记忆与秒级召回
                    </h4>
                    <p className="text-xs text-slate-500 dark:text-slate-400 leading-normal">
                      支持精准全文 FTS5 检索、本地 Ollama 向量检索以及那年今日重温，助你秒级找回记忆碎片。
                    </p>
                  </div>
                </div>
              </div>

              <div className="pt-2 flex justify-end">
                <button
                  type="button"
                  onClick={() => setCurrentStep('discovery')}
                  className="px-6 py-3 rounded-2xl bg-blue-600 hover:bg-blue-700 text-white font-semibold text-sm shadow-md hover:shadow-lg transition flex items-center gap-2 cursor-pointer"
                >
                  <span>开始配置我的记忆库</span>
                  <ArrowRight className="w-4 h-4" />
                </button>
              </div>
            </div>
          )}

          {/* Step 2: Browser Discovery */}
          {currentStep === 'discovery' && (
            <div className="space-y-6 animate-in fade-in duration-200">
              <div className="space-y-1">
                <h2 className="text-2xl font-black text-slate-900 dark:text-slate-100 tracking-tight">
                  自动发现本地浏览器
                </h2>
                <p className="text-sm text-slate-500 dark:text-slate-400">
                  系统已扫描你的操作系统，检测到以下已安装的浏览器配置文件：
                </p>
              </div>

              {isScanning ? (
                <div className="p-12 flex flex-col items-center justify-center gap-3 text-slate-400">
                  <RefreshCw className="w-6 h-6 animate-spin text-blue-500" />
                  <span className="text-xs">正在探测系统中安装的浏览器与 Profile...</span>
                </div>
              ) : profiles.length === 0 ? (
                <div className="p-8 text-center rounded-2xl bg-slate-50 dark:bg-slate-800/60 border border-slate-200 dark:border-slate-700 text-xs text-slate-500">
                  未自动检测到常见浏览器。你也可以在稍后的“数据源”页面手动选择历史文件导入。
                </div>
              ) : (
                <div className="space-y-2.5 max-h-60 overflow-y-auto pr-1">
                  {profiles.map((p, idx) => (
                    <div
                      key={idx}
                      className="p-3.5 rounded-2xl bg-slate-50 dark:bg-slate-800/70 border border-slate-200/80 dark:border-slate-700/80 flex items-center justify-between"
                    >
                      <div className="flex items-center gap-3 min-w-0 pr-2">
                        <div className="w-8 h-8 rounded-xl bg-blue-100/60 dark:bg-blue-950/60 flex items-center justify-center text-blue-600 dark:text-blue-400 shrink-0 font-bold text-xs uppercase">
                          {p.browser.slice(0, 2)}
                        </div>
                        <div className="min-w-0">
                          <div className="flex items-center gap-2">
                            <span className="font-bold text-xs text-slate-900 dark:text-slate-100 uppercase">
                              {p.browser}
                            </span>
                            <span className="text-[11px] px-1.5 py-0.2 rounded bg-slate-200 dark:bg-slate-700 text-slate-600 dark:text-slate-300 font-mono">
                              {p.profile_name}
                            </span>
                          </div>
                          <p className="text-[10px] text-slate-400 truncate max-w-sm font-mono mt-0.5">
                            {p.history_path}
                          </p>
                        </div>
                      </div>

                      <div className="text-right shrink-0">
                        <span className="text-xs font-mono text-slate-500 dark:text-slate-400">
                          {(p.history_size_bytes / (1024 * 1024)).toFixed(1)} MB
                        </span>
                      </div>
                    </div>
                  ))}
                </div>
              )}

              {/* Sync Action Area */}
              <div className="p-4 rounded-2xl bg-blue-50/70 dark:bg-blue-950/30 border border-blue-200/60 dark:border-blue-800/60 flex items-center justify-between">
                <div>
                  <h4 className="text-xs font-bold text-slate-900 dark:text-slate-100">
                    首次全量数据提取
                  </h4>
                  <p className="text-[11px] text-slate-500 dark:text-slate-400">
                    安全读取只读快照，不会影响浏览器正在进行的访问。
                  </p>
                </div>
                <button
                  type="button"
                  onClick={handleStartInitialSync}
                  disabled={isSyncing}
                  className="px-4 py-2 rounded-xl bg-blue-600 hover:bg-blue-700 disabled:opacity-50 text-white text-xs font-semibold transition flex items-center gap-2 shrink-0 cursor-pointer"
                >
                  <RefreshCw className={`w-3.5 h-3.5 ${isSyncing ? 'animate-spin' : ''}`} />
                  <span>
                    {isSyncing
                      ? '正在提取归档...'
                      : syncedCount !== null
                      ? `已导入 +${syncedCount} 条`
                      : '立即开始首次导入'}
                  </span>
                </button>
              </div>

              <div className="flex items-center justify-between pt-2">
                <button
                  type="button"
                  onClick={() => setCurrentStep('welcome')}
                  className="text-xs text-slate-400 hover:text-slate-600 dark:hover:text-slate-300"
                >
                  返回上一步
                </button>
                <button
                  type="button"
                  onClick={() => setCurrentStep('security')}
                  className="px-6 py-3 rounded-2xl bg-blue-600 hover:bg-blue-700 text-white font-semibold text-sm shadow-md hover:shadow-lg transition flex items-center gap-2 cursor-pointer"
                >
                  <span>下一步：安全应用锁</span>
                  <ArrowRight className="w-4 h-4" />
                </button>
              </div>
            </div>
          )}

          {/* Step 3: Security PIN */}
          {currentStep === 'security' && (
            <div className="space-y-6 animate-in fade-in duration-200">
              <div className="space-y-1">
                <h2 className="text-2xl font-black text-slate-900 dark:text-slate-100 tracking-tight">
                  PIN 应用锁（可选）
                </h2>
                <p className="text-sm text-slate-500 dark:text-slate-400">
                  PIN 只锁定应用窗口，防止旁人看到界面。浏览历史仍以明文保存在本机 SQLite 中，PIN 不会加密数据库。
                </p>
              </div>

              {!recoveryKey ? (
                <form onSubmit={handleSavePin} className="space-y-4">
                  <div className="grid grid-cols-2 gap-4">
                    <div className="space-y-1.5">
                      <label className="text-xs font-semibold text-slate-700 dark:text-slate-300">
                        设置 4-6 位数字 PIN
                      </label>
                      <input
                        type="password"
                        value={pin}
                        onChange={(e) => setPin(e.target.value)}
                        placeholder="••••"
                        maxLength={6}
                        className="w-full px-4 py-3 rounded-xl bg-slate-50 dark:bg-slate-800 border border-slate-200 dark:border-slate-700 font-mono text-center tracking-widest text-lg outline-none focus:border-blue-500 transition"
                      />
                    </div>

                    <div className="space-y-1.5">
                      <label className="text-xs font-semibold text-slate-700 dark:text-slate-300">
                        确认 PIN 码
                      </label>
                      <input
                        type="password"
                        value={confirmPin}
                        onChange={(e) => setConfirmPin(e.target.value)}
                        placeholder="••••"
                        maxLength={6}
                        className="w-full px-4 py-3 rounded-xl bg-slate-50 dark:bg-slate-800 border border-slate-200 dark:border-slate-700 font-mono text-center tracking-widest text-lg outline-none focus:border-blue-500 transition"
                      />
                    </div>
                  </div>

                  {pinError && (
                    <p className="text-xs text-rose-500 font-medium">{pinError}</p>
                  )}

                  <div className="pt-2 flex items-center justify-between">
                    <button
                      type="button"
                      onClick={() => setCurrentStep('ready')}
                      className="text-xs text-slate-500 hover:text-slate-700 dark:text-slate-400 dark:hover:text-slate-200"
                    >
                      暂不设置，稍后在设置中开启
                    </button>
                    <button
                      type="submit"
                      disabled={settingPin || !pin}
                      className="px-5 py-2.5 rounded-xl bg-blue-600 hover:bg-blue-700 disabled:opacity-50 text-white font-semibold text-xs shadow-sm transition flex items-center gap-2 cursor-pointer"
                    >
                      <Lock className="w-3.5 h-3.5" />
                      <span>{settingPin ? '正在启用应用锁...' : '启用 PIN 应用锁'}</span>
                    </button>
                  </div>
                </form>
              ) : (
                <div className="space-y-4 p-5 rounded-2xl bg-emerald-50/70 dark:bg-emerald-950/40 border border-emerald-200 dark:border-emerald-800">
                  <div className="flex items-center gap-2 text-emerald-600 dark:text-emerald-400">
                    <CheckCircle2 className="w-5 h-5" />
                    <span className="font-bold text-sm">PIN 码保护已生效</span>
                  </div>

                  <p className="text-xs text-slate-600 dark:text-slate-300 leading-relaxed">
                    请务必保存以下<strong>恢复密钥</strong>。忘记 PIN 时可用它重置应用锁。密钥不能加密或解密历史库，数据始终以明文存放在本机。
                  </p>

                  <div className="flex items-center justify-between p-3 bg-white dark:bg-slate-900 rounded-xl border border-emerald-200 dark:border-emerald-800 font-mono text-xs text-emerald-700 dark:text-emerald-300 select-all">
                    <span>{recoveryKey}</span>
                    <button
                      type="button"
                      onClick={handleCopyRecoveryKey}
                      className="p-1.5 rounded-lg hover:bg-slate-100 dark:hover:bg-slate-800 text-slate-500 hover:text-slate-800 dark:hover:text-slate-200 transition"
                      title="复制密钥"
                    >
                      {copiedKey ? <Check className="w-4 h-4 text-emerald-500" /> : <Copy className="w-4 h-4" />}
                    </button>
                  </div>

                  <div className="flex justify-end pt-2">
                    <button
                      type="button"
                      onClick={() => setCurrentStep('ready')}
                      className="px-6 py-2.5 rounded-xl bg-emerald-600 hover:bg-emerald-700 text-white text-xs font-semibold transition cursor-pointer"
                    >
                      我已妥善保存，继续
                    </button>
                  </div>
                </div>
              )}

              {!recoveryKey && (
                <div className="flex items-center justify-between pt-2 border-t border-slate-100 dark:border-slate-800">
                  <button
                    type="button"
                    onClick={() => setCurrentStep('discovery')}
                    className="text-xs text-slate-400 hover:text-slate-600 dark:hover:text-slate-300"
                  >
                    返回上一步
                  </button>
                </div>
              )}
            </div>
          )}

          {/* Step 4: Ready to explore */}
          {currentStep === 'ready' && (
            <div className="space-y-6 animate-in fade-in duration-200 text-center py-4">
              <div className="w-16 h-16 rounded-3xl bg-blue-50 dark:bg-blue-950/80 border border-blue-200 dark:border-blue-800/80 flex items-center justify-center text-blue-600 dark:text-blue-400 mx-auto shadow-sm">
                <Compass className="w-8 h-8" />
              </div>

              <div className="space-y-1.5">
                <h2 className="text-2xl font-black text-slate-900 dark:text-slate-100 tracking-tight">
                  一切就绪，开启你的网络记忆之旅！
                </h2>
                <p className="text-sm text-slate-500 dark:text-slate-400 max-w-md mx-auto">
                  Browsory 已经在后台守护你的浏览历史，并在系统托盘保持静默运行。
                </p>
              </div>

              {/* Tips Grid */}
              <div className="grid grid-cols-2 gap-3 text-left max-w-md mx-auto">
                <div className="p-3 rounded-xl bg-slate-50 dark:bg-slate-800/60 border border-slate-200/70 dark:border-slate-800 space-y-1">
                  <div className="flex items-center gap-1.5 text-xs font-bold text-slate-800 dark:text-slate-200">
                    <Search className="w-3.5 h-3.5 text-blue-500" />
                    <span>全局快速搜索</span>
                  </div>
                  <p className="text-[11px] text-slate-400">
                    随时按下 <kbd className="font-mono bg-slate-200 dark:bg-slate-700 px-1 rounded">Ctrl+K</kbd> 秒级呼出 Spotlight 查找记忆。
                  </p>
                </div>

                <div className="p-3 rounded-xl bg-slate-50 dark:bg-slate-800/60 border border-slate-200/70 dark:border-slate-800 space-y-1">
                  <div className="flex items-center gap-1.5 text-xs font-bold text-slate-800 dark:text-slate-200">
                    <Globe className="w-3.5 h-3.5 text-emerald-500" />
                    <span>托盘常驻守护</span>
                  </div>
                  <p className="text-[11px] text-slate-400">
                    关闭窗口自动收起至右下角托盘，日常自动增量备份。
                  </p>
                </div>
              </div>

              <div className="pt-4">
                <button
                  type="button"
                  onClick={finishOnboarding}
                  className="px-8 py-3.5 rounded-2xl bg-blue-600 hover:bg-blue-700 text-white font-bold text-sm shadow-lg hover:shadow-xl transition transform hover:scale-[1.02] cursor-pointer"
                >
                  进入我的网络记忆库
                </button>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
};
