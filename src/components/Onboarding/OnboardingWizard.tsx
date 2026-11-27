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
import { useDialogFocus } from '../../hooks/useDialogFocus';
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
  const { showToast, t, language } = useAppStore();
  const ui = (zh: string, en: string) => (language === 'en-US' ? en : zh);
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

  const dialogRef = useDialogFocus<HTMLDivElement>({
    isOpen,
    onClose: () => {
      if (currentStep === 'ready' && !settingPin) {
        finishOnboarding();
      }
    },
  });

  // Load browser discovery when reaching discovery step
  useEffect(() => {
    if (isOpen && currentStep === 'discovery') {
      loadProfiles();
    }
  }, [isOpen, currentStep]);

  if (!isOpen) return null;

  const loadProfiles = async () => {
    setIsScanning(true);
    try {
      const list = await tauriApi.scanBrowsers();
      setProfiles(list);
    } catch (e) {
      console.error('Failed to scan browsers during onboarding:', e);
    } finally {
      setIsScanning(false);
    }
  };

  const handleStartInitialSync = async () => {
    setIsSyncing(true);
    try {
      const count = await triggerSyncAll();
      setSyncedCount(count);
      showToast(ui(`首次数据同步成功，新增 ${count} 条历史足迹`, `Initial sync complete: added ${count} history records`), 'success');
    } catch (err: any) {
      showToast(typeof err === 'string' ? err : ui('部分浏览器可能处于开启状态，跳过锁定的文件', 'Some browser files may be locked and were skipped'), 'warning');
    } finally {
      setIsSyncing(false);
    }
  };

  const handleSavePin = async (e: React.FormEvent) => {
    e.preventDefault();
    setPinError('');

    if (!/^\d{4}$/.test(pin)) {
      setPinError(ui('PIN 码必须为 4 位纯数字', 'PIN must be 4 numeric digits'));
      return;
    }

    if (pin !== confirmPin) {
      setPinError(ui('两次输入的 PIN 码不一致', 'The two PIN entries do not match'));
      return;
    }

    setSettingPin(true);
    try {
      const key = await setOrChangePin(null, pin);
      setRecoveryKey(key);
      showToast(ui('应用安全 PIN 码设置成功', 'Screen PIN lock enabled'), 'success');
    } catch (err) {
      setPinError(typeof err === 'string' ? err : ui('设置 PIN 码失败', 'Failed to set PIN'));
    } finally {
      setSettingPin(false);
    }
  };

  const handleCopyRecoveryKey = async () => {
    if (!recoveryKey) return;
    try {
      await navigator.clipboard.writeText(recoveryKey);
      setCopiedKey(true);
      setTimeout(() => setCopiedKey(false), 2000);
      showToast(t('detail.copied'), 'success');
    } catch {
      showToast(t('detail.copyFailed'), 'error');
    }
  };

  const finishOnboarding = () => {
    localStorage.setItem('browsory_onboarded_v1', 'true');
    onComplete();
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/75 backdrop-blur-md p-4 animate-in fade-in duration-300 select-none">
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="onboarding-title"
        tabIndex={-1}
        className="w-full max-w-2xl bg-white dark:bg-slate-900 border border-slate-200 dark:border-slate-800 rounded-3xl shadow-2xl overflow-hidden flex flex-col max-h-[90vh]"
      >
        {/* Step Indicator Top Bar */}
        <div className="px-8 pt-6 pb-4 border-b border-slate-100 dark:border-slate-800 flex items-center justify-between">
          <div className="flex items-center gap-2.5">
            <AppLogo size={24} />
            <span id="onboarding-title" className="font-bold text-sm tracking-tight text-slate-900 dark:text-slate-100">
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
        <div className="p-8 overflow-y-auto modal-scroll flex-1">
          {/* Step 1: Welcome & Privacy */}
          {currentStep === 'welcome' && (
            <div className="space-y-6 animate-in fade-in duration-200">
              <div className="space-y-2">
                <span className="inline-flex items-center gap-1.5 px-3 py-1 rounded-full text-xs font-semibold bg-blue-50 dark:bg-blue-950 text-blue-600 dark:text-blue-400 border border-blue-200 dark:border-blue-800">
                  <Sparkles className="w-3.5 h-3.5" />
                  {ui('欢迎使用 Browsory', 'Welcome to Browsory')}
                </span>
                <h2 className="text-2xl font-black text-slate-900 dark:text-slate-100 tracking-tight">
                  {ui('你的私人本地网络记忆库', 'Your private local-first browsing archive')}
                </h2>
                <p className="text-sm text-slate-500 dark:text-slate-400 leading-relaxed">
                  {ui(
                    'Browsory 帮助你自动汇聚散落在各个浏览器的历史足迹，打破设备与平台隔阂，并提供深度检索、智能重聚与记忆重温能力。',
                    'Browsory consolidates browsing trails across browsers into a single encrypted local archive with fast full-text search and recall.'
                  )}
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
                      {ui('100% 本地优先 (Local-First)', '100% Local-First')}
                    </h4>
                    <p className="text-xs text-slate-500 dark:text-slate-400 leading-normal">
                      {ui(
                        '所有浏览历史保存在本机独立加密数据库保险库中。无需云端账号，无后台隐秘追踪。',
                        'All history stays in an encrypted local SQLCipher vault with no cloud account and no hidden telemetry.'
                      )}
                    </p>
                  </div>
                </div>

                <div className="p-4 rounded-2xl bg-slate-50 dark:bg-slate-800/60 border border-slate-200/80 dark:border-slate-800 flex items-start gap-3.5">
                  <div className="p-2 rounded-xl bg-emerald-100 dark:bg-emerald-950/80 text-emerald-600 dark:text-emerald-400 shrink-0">
                    <Database className="w-5 h-5" />
                  </div>
                  <div className="space-y-0.5">
                    <h4 className="text-sm font-bold text-slate-900 dark:text-slate-100">
                      {ui('数据永久属于你，无保留期限限制', 'Permanent ownership without browser expiration')}
                    </h4>
                    <p className="text-xs text-slate-500 dark:text-slate-400 leading-normal">
                      {ui(
                        '不同于普通浏览器仅保留有限天数，Browsory 持久留存你的历史足迹，不设置数据量或天数门槛。',
                        'Unlike browsers that expire old records after a few months, Browsory retains your local archive indefinitely.'
                      )}
                    </p>
                  </div>
                </div>

                <div className="p-4 rounded-2xl bg-slate-50 dark:bg-slate-800/60 border border-slate-200/80 dark:border-slate-800 flex items-start gap-3.5">
                  <div className="p-2 rounded-xl bg-blue-100 dark:bg-blue-950/80 text-blue-600 dark:text-blue-400 shrink-0">
                    <Sparkles className="w-5 h-5" />
                  </div>
                  <div className="space-y-0.5">
                    <h4 className="text-sm font-bold text-slate-900 dark:text-slate-100">
                      {ui('语义检索与那年今日重温', 'Semantic recall and On This Day')}
                    </h4>
                    <p className="text-xs text-slate-500 dark:text-slate-400 leading-normal">
                      {ui(
                        '支持精准全文 FTS5 检索、可选本地 Ollama 或兼容 AI 接口向量检索，助你快速找回记忆。',
                        'Combines instant FTS5 full-text filtering with optional local Ollama or OpenAI-compatible embeddings.'
                      )}
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
                  <span>{ui('开始配置我的记忆库', 'Configure my archive')}</span>
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
                  {ui('自动发现本地浏览器', 'Discover installed browsers')}
                </h2>
                <p className="text-sm text-slate-500 dark:text-slate-400">
                  {ui('系统已扫描本机，检测到以下浏览器配置文件：', 'Detected browser profiles on this machine:')}
                </p>
              </div>

              {isScanning ? (
                <div className="p-12 flex flex-col items-center justify-center gap-3 text-slate-400">
                  <RefreshCw className="w-6 h-6 animate-spin text-blue-500" />
                  <span className="text-xs">{ui('正在探测系统中安装的浏览器与 Profile...', 'Scanning installed browsers and profiles...')}</span>
                </div>
              ) : profiles.length === 0 ? (
                <div className="p-8 text-center rounded-2xl bg-slate-50 dark:bg-slate-800/60 border border-slate-200 dark:border-slate-700 text-xs text-slate-500">
                  {ui(
                    '未自动检测到常见浏览器。你也可以在稍后的“数据源”页面手动选择历史文件导入。',
                    'No standard browser profiles detected automatically. You can import history files manually from the Sources tab.'
                  )}
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
                          <p className="text-[11px] text-slate-400 truncate max-w-sm font-mono mt-0.5">
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
              <div className="p-4 rounded-2xl bg-blue-50/70 dark:bg-blue-950/30 border border-blue-200/60 dark:border-blue-800/60 flex items-center justify-between gap-3">
                <div>
                  <h4 className="text-xs font-bold text-slate-900 dark:text-slate-100">
                    {ui('首次全量数据提取', 'Initial history snapshot sync')}
                  </h4>
                  <p className="text-xs text-slate-500 dark:text-slate-400">
                    {ui('安全读取只读快照，不会影响浏览器正在进行的访问。', 'Reads via temporary snapshots without interrupting open browsers.')}
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
                      ? ui('正在提取归档...', 'Syncing archive...')
                      : syncedCount !== null
                      ? ui(`已导入 +${syncedCount} 条`, `Imported +${syncedCount}`)
                      : ui('立即开始首次导入', 'Start initial sync')}
                  </span>
                </button>
              </div>

              <div className="flex items-center justify-between pt-2">
                <button
                  type="button"
                  onClick={() => setCurrentStep('welcome')}
                  className="text-xs text-slate-400 hover:text-slate-600 dark:hover:text-slate-300 cursor-pointer"
                >
                  {ui('返回上一步', 'Back')}
                </button>
                <button
                  type="button"
                  onClick={() => setCurrentStep('security')}
                  className="px-6 py-3 rounded-2xl bg-blue-600 hover:bg-blue-700 text-white font-semibold text-sm shadow-md hover:shadow-lg transition flex items-center gap-2 cursor-pointer"
                >
                  <span>{ui('下一步：安全应用锁', 'Next: App PIN lock')}</span>
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
                  {ui('PIN 应用锁（可选）', 'App PIN lock (optional)')}
                </h2>
                <p className="text-sm text-slate-500 dark:text-slate-400">
                  {ui(
                    'PIN 只锁定应用窗口，防止旁人看到界面。数据库由独立的保险库主口令保护；PIN 与数据库加密互不替代。',
                    'PIN hides the application window when you step away. Database files remain protected by your separate vault passphrase.'
                  )}
                </p>
              </div>

              {!recoveryKey ? (
                <form onSubmit={handleSavePin} className="space-y-4">
                  <div className="grid grid-cols-2 gap-4">
                    <div className="space-y-1.5">
                      <label className="text-xs font-semibold text-slate-700 dark:text-slate-300">
                        {ui('设置 4 位数字 PIN', 'Set 4-digit PIN')}
                      </label>
                      <input
                        type="password"
                        inputMode="numeric"
                        value={pin}
                        onChange={(e) => setPin(e.target.value.replace(/\D/g, '').slice(0, 4))}
                        placeholder="••••"
                        maxLength={4}
                        className="w-full px-4 py-3 rounded-xl bg-slate-50 dark:bg-slate-800 border border-slate-200 dark:border-slate-700 font-mono text-center tracking-widest text-lg outline-none focus:border-blue-500 transition"
                      />
                    </div>

                    <div className="space-y-1.5">
                      <label className="text-xs font-semibold text-slate-700 dark:text-slate-300">
                        {ui('确认 PIN 码', 'Confirm PIN')}
                      </label>
                      <input
                        type="password"
                        inputMode="numeric"
                        value={confirmPin}
                        onChange={(e) => setConfirmPin(e.target.value.replace(/\D/g, '').slice(0, 4))}
                        placeholder="••••"
                        maxLength={4}
                        className="w-full px-4 py-3 rounded-xl bg-slate-50 dark:bg-slate-800 border border-slate-200 dark:border-slate-700 font-mono text-center tracking-widest text-lg outline-none focus:border-blue-500 transition"
                      />
                    </div>
                  </div>

                  {pinError && (
                    <p role="alert" className="text-xs text-rose-500 font-medium">{pinError}</p>
                  )}

                  <div className="pt-2 flex items-center justify-between">
                    <button
                      type="button"
                      onClick={() => setCurrentStep('ready')}
                      className="text-xs text-slate-500 hover:text-slate-700 dark:text-slate-400 dark:hover:text-slate-200 cursor-pointer"
                    >
                      {ui('暂不设置，稍后在设置中开启', 'Skip for now')}
                    </button>
                    <button
                      type="submit"
                      disabled={settingPin || !pin}
                      className="px-5 py-2.5 rounded-xl bg-blue-600 hover:bg-blue-700 disabled:opacity-50 text-white font-semibold text-xs shadow-sm transition flex items-center gap-2 cursor-pointer"
                    >
                      <Lock className="w-3.5 h-3.5" />
                      <span>{settingPin ? ui('正在启用应用锁...', 'Enabling PIN lock...') : ui('启用 PIN 应用锁', 'Enable PIN lock')}</span>
                    </button>
                  </div>
                </form>
              ) : (
                <div className="space-y-4 p-5 rounded-2xl bg-emerald-50/70 dark:bg-emerald-950/40 border border-emerald-200 dark:border-emerald-800">
                  <div className="flex items-center gap-2 text-emerald-600 dark:text-emerald-400">
                    <CheckCircle2 className="w-5 h-5" />
                    <span className="font-bold text-sm">{ui('PIN 码保护已生效', 'PIN lock is active')}</span>
                  </div>

                  <p className="text-xs text-slate-600 dark:text-slate-300 leading-relaxed">
                    {ui(
                      '请务必保存以下恢复密钥。忘记 PIN 时可用它重置应用锁；数据库恢复仍需要保险库主口令。',
                      'Save this PIN recovery key in a safe place to reset your PIN if forgotten. Database recovery still uses your vault passphrase.'
                    )}
                  </p>

                  <div className="flex items-center justify-between p-3 bg-white dark:bg-slate-900 rounded-xl border border-emerald-200 dark:border-emerald-800 font-mono text-xs text-emerald-700 dark:text-emerald-300 select-all">
                    <span>{recoveryKey}</span>
                    <button
                      type="button"
                      onClick={handleCopyRecoveryKey}
                      className="p-1.5 rounded-lg hover:bg-slate-100 dark:hover:bg-slate-800 text-slate-500 hover:text-slate-800 dark:hover:text-slate-200 transition cursor-pointer"
                      aria-label={ui('复制密钥', 'Copy key')}
                      title={ui('复制密钥', 'Copy key')}
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
                      {ui('我已妥善保存，继续', 'I saved it, continue')}
                    </button>
                  </div>
                </div>
              )}

              {!recoveryKey && (
                <div className="flex items-center justify-between pt-2 border-t border-slate-100 dark:border-slate-800">
                  <button
                    type="button"
                    onClick={() => setCurrentStep('discovery')}
                    className="text-xs text-slate-400 hover:text-slate-600 dark:hover:text-slate-300 cursor-pointer"
                  >
                    {ui('返回上一步', 'Back')}
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
                  {ui('一切就绪，开启你的网络记忆之旅！', 'All set! Your local history archive is ready')}
                </h2>
                <p className="text-sm text-slate-500 dark:text-slate-400 max-w-md mx-auto">
                  {ui(
                    'Browsory 已经在本地就绪，你可以随时浏览历史、检索记录或配置安全策略。',
                    'Browsory is ready locally to browse history, run fast searches, and protect your archive.'
                  )}
                </p>
              </div>

              {/* Tips Grid */}
              <div className="grid grid-cols-2 gap-3 text-left max-w-md mx-auto">
                <div className="p-3 rounded-xl bg-slate-50 dark:bg-slate-800/60 border border-slate-200/70 dark:border-slate-800 space-y-1">
                  <div className="flex items-center gap-1.5 text-xs font-bold text-slate-800 dark:text-slate-200">
                    <Search className="w-3.5 h-3.5 text-blue-500" />
                    <span>{ui('全局快速搜索', 'Quick search')}</span>
                  </div>
                  <p className="text-xs text-slate-400">
                    {ui('随时按下 Ctrl+K 呼出快速搜索框。', 'Press Ctrl+K anytime to open Quick Search.')}
                  </p>
                </div>

                <div className="p-3 rounded-xl bg-slate-50 dark:bg-slate-800/60 border border-slate-200/70 dark:border-slate-800 space-y-1">
                  <div className="flex items-center gap-1.5 text-xs font-bold text-slate-800 dark:text-slate-200">
                    <Globe className="w-3.5 h-3.5 text-emerald-500" />
                    <span>{ui('托盘常驻守护', 'Tray background watch')}</span>
                  </div>
                  <p className="text-xs text-slate-400">
                    {ui('支持最小化至系统托盘并自动同步新增足迹。', 'Minimize to tray to keep local snapshots updated.')}
                  </p>
                </div>
              </div>

              <div className="pt-4">
                <button
                  type="button"
                  onClick={finishOnboarding}
                  className="px-8 py-3.5 rounded-2xl bg-blue-600 hover:bg-blue-700 text-white font-bold text-sm shadow-lg hover:shadow-xl transition transform hover:scale-[1.02] cursor-pointer"
                >
                  {ui('进入我的网络记忆库', 'Open Browsory')}
                </button>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
};
