import React, { useEffect, useState } from 'react';
import {
  Sparkles,
  ChevronDown,
  ChevronUp,
  Loader2,
  CheckCircle2,
  AlertCircle,
  FolderOpen,
  Languages,
  Sun,
  Moon,
  Settings as SettingsIcon,
  FolderTree,
} from 'lucide-react';
import { useAppStore } from '../../stores/useAppStore';
import { tauriApi } from '../../services/tauri';
import { AppInfo } from '../../types';
import { DiagnosticsCard } from './DiagnosticsCard';
import { StorageSettingsCard } from './StorageSettingsCard';
import { AboutCard } from './AboutCard';
import { EmbeddingSettingsCard } from './EmbeddingSettingsCard';
import { PresetManager, PresetItem } from './PresetManager';
import { LicenseSettingsCard } from './LicenseSettingsCard';
import { WebdavSyncCard } from './WebdavSyncCard';

export const SettingsPage: React.FC = () => {
  const { language, setLanguage, theme, setTheme, showToast, t } = useAppStore();
  const [appInfo, setAppInfo] = useState<AppInfo | null>(null);
  const [closeBehavior, setCloseBehavior] = useState<'ask' | 'tray' | 'quit'>('ask');

  const isCurrentDark =
    theme === 'dark' ||
    (theme === 'system' &&
      typeof window !== 'undefined' &&
      window.matchMedia('(prefers-color-scheme: dark)').matches);

  const toggleTheme = () => {
    setTheme(isCurrentDark ? 'light' : 'dark');
  };

  const [isAiExpanded, setIsAiExpanded] = useState(false);
  const [providerType, setProviderType] = useState<'cloud' | 'local'>('cloud');
  const [aiBaseUrl, setAiBaseUrl] = useState('https://api.openai.com/v1');
  const [aiKey, setAiKey] = useState('');
  const [aiModel, setAiModel] = useState('gpt-4o-mini');

  const [cloudPresets, setCloudPresets] = useState<PresetItem[]>([]);
  const [localPresets, setLocalPresets] = useState<PresetItem[]>([]);

  const [isTesting, setIsTesting] = useState(false);
  const [testFeedback, setTestFeedback] = useState<{
    type: 'success' | 'error';
    message: string;
  } | null>(null);

  useEffect(() => {
    tauriApi.getAppInfo().then(setAppInfo).catch(console.error);
    tauriApi.getSetting('close_behavior').then((v) => {
      if (v === 'tray' || v === 'quit' || v === 'ask') setCloseBehavior(v);
    });
    tauriApi.getSetting('ai_base_url').then((v) => {
      if (v) {
        setAiBaseUrl(v);
        if (v.includes('localhost') || v.includes('127.0.0.1')) {
          setProviderType('local');
        } else {
          setProviderType('cloud');
        }
      }
    });
    tauriApi.getSetting('ai_model').then((v) => v && setAiModel(v));
    tauriApi.getSetting('ai_key').then((v) => v && setAiKey(v));

    tauriApi.getSetting('ai_presets_cloud').then((v) => {
      if (v) {
        try {
          const parsed = JSON.parse(v);
          if (Array.isArray(parsed)) setCloudPresets(parsed.slice(0, 3));
        } catch {}
      }
    });
    tauriApi.getSetting('ai_presets_local').then((v) => {
      if (v) {
        try {
          const parsed = JSON.parse(v);
          if (Array.isArray(parsed)) setLocalPresets(parsed.slice(0, 3));
        } catch {}
      }
    });
  }, []);

  const handleSelectProvider = (type: 'cloud' | 'local') => {
    setProviderType(type);
    setTestFeedback(null);
    if (type === 'local') {
      if (!aiBaseUrl.includes('localhost') && !aiBaseUrl.includes('127.0.0.1')) {
        setAiBaseUrl('http://localhost:11434/v1');
        setAiModel('qwen2.5:7b');
      }
    } else {
      if (aiBaseUrl.includes('localhost') || aiBaseUrl.includes('127.0.0.1')) {
        setAiBaseUrl('https://api.openai.com/v1');
        setAiModel('gpt-4o-mini');
      }
    }
  };

  const handleApplyPreset = (preset: PresetItem) => {
    if (preset.baseUrl) setAiBaseUrl(preset.baseUrl);
    if (preset.model) setAiModel(preset.model);
    if (preset.apiKey !== undefined) setAiKey(preset.apiKey);
    setTestFeedback(null);
  };

  const handleAddPreset = async (preset: PresetItem) => {
    if (providerType === 'cloud') {
      if (cloudPresets.length >= 3) {
        showToast('OpenAI兼容预设最多允许添加 3 个', 'warning');
        return;
      }
      const next = [...cloudPresets, preset].slice(0, 3);
      setCloudPresets(next);
      await tauriApi.setSetting('ai_presets_cloud', JSON.stringify(next));
      showToast(`已保存预设「${preset.name}」`, 'success');
    } else {
      if (localPresets.length >= 3) {
        showToast('本地模型预设最多允许添加 3 个', 'warning');
        return;
      }
      const next = [...localPresets, preset].slice(0, 3);
      setLocalPresets(next);
      await tauriApi.setSetting('ai_presets_local', JSON.stringify(next));
      showToast(`已保存预设「${preset.name}」`, 'success');
    }
  };

  const handleDeletePreset = async (index: number) => {
    if (providerType === 'cloud') {
      const next = cloudPresets.filter((_, i) => i !== index);
      setCloudPresets(next);
      await tauriApi.setSetting('ai_presets_cloud', JSON.stringify(next));
      showToast('预设已删除', 'info');
    } else {
      const next = localPresets.filter((_, i) => i !== index);
      setLocalPresets(next);
      await tauriApi.setSetting('ai_presets_local', JSON.stringify(next));
      showToast('预设已删除', 'info');
    }
  };

  const handleTestAI = async () => {
    setIsTesting(true);
    setTestFeedback(null);
    try {
      const res = await tauriApi.testAiConnection({
        base_url: aiBaseUrl,
        api_key: aiKey || undefined,
        model: aiModel,
      });
      setTestFeedback({
        type: res.success ? 'success' : 'error',
        message: res.message,
      });
      if (res.success) {
        showToast('AI 端点连接测试成功！', 'success');
      }
    } catch (err: any) {
      const msg = typeof err === 'string' ? err : err?.message || '连接失败，无法访问 AI 服务';
      setTestFeedback({
        type: 'error',
        message: msg,
      });
      showToast('AI 端点连接测试失败', 'error');
    } finally {
      setIsTesting(false);
    }
  };

  const handleSaveAI = async () => {
    await tauriApi.setSetting('ai_base_url', aiBaseUrl);
    await tauriApi.setSetting('ai_model', aiModel);
    await tauriApi.setSetting('ai_key', aiKey);
    showToast('AI 配置已保存', 'success');
  };

  const formatBytes = (bytes: number) => {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
  };

  return (
    <div className="flex flex-col h-full bg-slate-50 dark:bg-slate-950 overflow-y-auto select-none">
      {/* Top Banner Header - Matching PrivacyPage layout */}
      <div className="p-6 md:p-8 bg-white dark:bg-slate-900 border-b border-slate-200 dark:border-slate-800 shrink-0">
        <div className="max-w-6xl mx-auto flex flex-col md:flex-row md:items-center justify-between gap-4">
          <div className="flex items-center gap-4">
            <div className="p-3.5 rounded-2xl bg-gradient-to-br from-blue-600 to-indigo-700 text-white shadow-lg shadow-blue-500/20">
              <SettingsIcon className="w-8 h-8" />
            </div>
            <div>
              <div className="flex items-center gap-2.5">
                <h1 className="text-xl md:text-2xl font-bold text-slate-900 dark:text-slate-100">
                  {t('settings.title')}
                </h1>
                <span className="px-2.5 py-0.5 text-xs font-semibold rounded-full bg-blue-100 dark:bg-blue-950/80 text-blue-700 dark:text-blue-300 border border-blue-300 dark:border-blue-800">
                  系统设置
                </span>
              </div>
              <p className="text-xs md:text-sm text-slate-500 dark:text-slate-400 mt-1">
                界面外观偏好、AI 智能分析端点、向量检索、多设备同步与物理存储管理。
              </p>
            </div>
          </div>

          <div className="flex items-center gap-2.5">
            {/* 深浅色模式切换按钮 */}
            <button
              type="button"
              onClick={toggleTheme}
              className="flex items-center gap-1.5 px-3 py-2 rounded-xl border border-slate-200 dark:border-slate-700 bg-white dark:bg-slate-800 hover:bg-slate-100 dark:hover:bg-slate-700/80 text-slate-700 dark:text-slate-200 text-xs font-medium transition shadow-xs cursor-pointer"
              title={isCurrentDark ? '切换为浅色模式' : '切换为深色模式'}
            >
              {isCurrentDark ? (
                <Sun className="w-3.5 h-3.5 text-amber-500" />
              ) : (
                <Moon className="w-3.5 h-3.5 text-slate-600" />
              )}
              <span>{isCurrentDark ? '深色模式' : '浅色模式'}</span>
            </button>

            {/* 界面语言切换按钮 */}
            <button
              type="button"
              onClick={() => setLanguage(language === 'zh-CN' ? 'en-US' : 'zh-CN')}
              className="flex items-center gap-1.5 px-3 py-2 rounded-xl border border-slate-200 dark:border-slate-700 bg-white dark:bg-slate-800 hover:bg-slate-100 dark:hover:bg-slate-700/80 text-slate-700 dark:text-slate-200 text-xs font-medium transition shadow-xs cursor-pointer"
              title="切换界面语言 (Switch Language)"
            >
              <Languages className="w-3.5 h-3.5 text-blue-500" />
              <span>{language === 'zh-CN' ? '中 / EN' : 'EN / 中'}</span>
            </button>
          </div>
        </div>
      </div>

      {/* Main Content Area - max-w-6xl matching PrivacyPage */}
      <div className="max-w-6xl mx-auto w-full p-6 md:p-8 space-y-8">
        {/* 1. AI API Configuration */}
        <div className="space-y-4">
          <h2 className="text-sm font-semibold uppercase tracking-wider text-slate-400">
            AI 智能分析与模型接口
          </h2>

          <div className="bg-white dark:bg-slate-900 rounded-2xl border border-slate-200 dark:border-slate-800 shadow-xs overflow-hidden transition-all">
            {/* Collapsible Header */}
            <button
              type="button"
              onClick={() => setIsAiExpanded(!isAiExpanded)}
              className="w-full px-5 py-4 flex items-center justify-between bg-slate-50/50 dark:bg-slate-850/50 hover:bg-slate-100/70 dark:hover:bg-slate-800 transition-colors cursor-pointer text-left"
            >
              <div className="flex items-center gap-3">
                <div className="p-2.5 rounded-xl bg-purple-500/10 text-purple-600 dark:text-purple-400 border border-purple-500/20">
                  <Sparkles className="w-5 h-5 shrink-0" />
                </div>
                <div className="flex flex-col">
                  <div className="flex items-center gap-2">
                    <span className="font-semibold text-slate-900 dark:text-slate-100 text-sm">
                      AI 大语言模型端点配置
                    </span>
                    <span className="text-[10px] px-2 py-0.5 rounded-full bg-purple-50 text-purple-700 dark:bg-purple-950/60 dark:text-purple-300 font-mono font-medium border border-purple-200 dark:border-purple-800">
                      {providerType === 'local' ? '本地模式' : '云端兼容'}
                    </span>
                  </div>
                  <span className="text-xs text-slate-500 dark:text-slate-400 font-mono mt-0.5 truncate max-w-md">
                    {aiModel ? `${aiModel} @ ${aiBaseUrl}` : '未配置模型端点'}
                  </span>
                </div>
              </div>

              <div className="flex items-center gap-2 text-slate-400">
                <span className="text-xs">{isAiExpanded ? '收起配置' : '展开配置'}</span>
                {isAiExpanded ? <ChevronUp className="w-4 h-4" /> : <ChevronDown className="w-4 h-4" />}
              </div>
            </button>

            {/* Collapsed Body */}
            {isAiExpanded && (
              <div className="p-5 border-t border-slate-100 dark:border-slate-800 flex flex-col gap-4 animate-in fade-in-50 duration-150">
                {/* Provider Mode Switcher */}
                <div className="flex flex-col gap-1.5">
                  <label className="text-slate-600 dark:text-slate-300 text-xs font-medium">接口提供商模式</label>
                  <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                    <button
                      type="button"
                      onClick={() => handleSelectProvider('cloud')}
                      className={`p-3 rounded-xl border text-left flex flex-col gap-0.5 transition cursor-pointer ${
                        providerType === 'cloud'
                          ? 'border-purple-500 bg-purple-50/50 dark:bg-purple-950/30 text-purple-700 dark:text-purple-300 font-medium ring-1 ring-purple-500/20'
                          : 'border-slate-200 dark:border-slate-800 bg-slate-50 dark:bg-slate-900 text-slate-600 dark:text-slate-400 hover:bg-slate-100 dark:hover:bg-slate-800'
                      }`}
                    >
                      <span className="text-xs font-semibold">OpenAI 兼容 (云端接口)</span>
                      <span className="text-[11px] opacity-80">支持 OpenAI、DeepSeek、Moonshot 等</span>
                    </button>

                    <button
                      type="button"
                      onClick={() => handleSelectProvider('local')}
                      className={`p-3 rounded-xl border text-left flex flex-col gap-0.5 transition cursor-pointer ${
                        providerType === 'local'
                          ? 'border-purple-500 bg-purple-50/50 dark:bg-purple-950/30 text-purple-700 dark:text-purple-300 font-medium ring-1 ring-purple-500/20'
                          : 'border-slate-200 dark:border-slate-800 bg-slate-50 dark:bg-slate-900 text-slate-600 dark:text-slate-400 hover:bg-slate-100 dark:hover:bg-slate-800'
                      }`}
                    >
                      <span className="text-xs font-semibold">本地模型 (Local LLM)</span>
                      <span className="text-[11px] opacity-80">支持本地运行的 Ollama、LM Studio 等离线引擎</span>
                    </button>
                  </div>
                </div>

                {/* Quick Presets */}
                <PresetManager
                  label={providerType === 'cloud' ? 'OpenAI兼容预设:' : '本地模型预设:'}
                  presets={providerType === 'cloud' ? cloudPresets : localPresets}
                  onApply={handleApplyPreset}
                  onAdd={handleAddPreset}
                  onDelete={handleDeletePreset}
                  currentModel={aiModel}
                  currentBaseUrl={aiBaseUrl}
                  currentApiKey={providerType === 'cloud' ? aiKey : undefined}
                  maxCount={3}
                />

                {/* Form Inputs */}
                <div className="flex flex-col gap-3">
                  <div className="flex flex-col gap-1">
                    <label className="text-slate-600 dark:text-slate-300 text-xs flex items-center justify-between">
                      <span className="font-medium">Base URL</span>
                      <span className="text-[10px] text-slate-400">兼容 OpenAI 标准的 /chat/completions</span>
                    </label>
                    <input
                      type="text"
                      value={aiBaseUrl}
                      onChange={(e) => setAiBaseUrl(e.target.value)}
                      placeholder={providerType === 'local' ? 'http://localhost:11434/v1' : 'https://api.openai.com/v1'}
                      className="px-3 py-2 text-xs rounded-xl border border-slate-200 dark:border-slate-800 bg-white dark:bg-slate-900 text-slate-800 dark:text-slate-200 font-mono focus:outline-hidden focus:ring-1 focus:ring-purple-500"
                    />
                  </div>

                  <div className="flex flex-col gap-1">
                    <label className="text-slate-600 dark:text-slate-300 text-xs flex items-center justify-between">
                      <span className="font-medium">API Key {providerType === 'local' && <span className="text-emerald-600 dark:text-emerald-400 font-normal">(本地模型免 Key，可留空)</span>}</span>
                    </label>
                    <input
                      type="password"
                      value={aiKey}
                      onChange={(e) => setAiKey(e.target.value)}
                      placeholder={providerType === 'local' ? '本地模式无需 Key (可留空)' : 'sk-...'}
                      className="px-3 py-2 text-xs rounded-xl border border-slate-200 dark:border-slate-800 bg-white dark:bg-slate-900 text-slate-800 dark:text-slate-200 font-mono focus:outline-hidden focus:ring-1 focus:ring-purple-500"
                    />
                  </div>

                  <div className="flex flex-col gap-1">
                    <label className="text-slate-600 dark:text-slate-300 text-xs font-medium">模型名称 (Model Name)</label>
                    <input
                      type="text"
                      value={aiModel}
                      onChange={(e) => setAiModel(e.target.value)}
                      placeholder={providerType === 'local' ? 'qwen2.5:7b 或 deepseek-r1' : 'gpt-4o-mini 或 deepseek-chat'}
                      className="px-3 py-2 text-xs rounded-xl border border-slate-200 dark:border-slate-800 bg-white dark:bg-slate-900 text-slate-800 dark:text-slate-200 font-mono focus:outline-hidden focus:ring-1 focus:ring-purple-500"
                    />
                  </div>
                </div>

                {/* Action Buttons & Test Status */}
                <div className="flex items-center gap-3 pt-2">
                  <button
                    type="button"
                    onClick={handleSaveAI}
                    className="px-4 py-2 bg-purple-600 hover:bg-purple-700 text-white rounded-xl font-medium transition shadow-xs text-xs cursor-pointer"
                  >
                    保存 AI 配置
                  </button>

                  <button
                    type="button"
                    onClick={handleTestAI}
                    disabled={isTesting}
                    className="flex items-center gap-1.5 px-3.5 py-2 rounded-xl border border-slate-200 dark:border-slate-700 hover:bg-slate-100 dark:hover:bg-slate-800 text-slate-700 dark:text-slate-200 transition disabled:opacity-50 text-xs cursor-pointer"
                  >
                    {isTesting ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Sparkles className="w-3.5 h-3.5" />}
                    <span>{isTesting ? '正在测试...' : '测试连接'}</span>
                  </button>
                </div>

                {/* Test Result Feedback Alert */}
                {testFeedback && (
                  <div
                    className={`p-3.5 rounded-xl text-xs flex items-start gap-2.5 border ${
                      testFeedback.type === 'success'
                        ? 'bg-emerald-50 text-emerald-800 dark:bg-emerald-950/40 dark:text-emerald-300 border-emerald-200 dark:border-emerald-900'
                        : 'bg-rose-50 text-rose-800 dark:bg-rose-950/40 dark:text-rose-300 border-rose-200 dark:border-rose-900'
                    }`}
                  >
                    {testFeedback.type === 'success' ? (
                      <CheckCircle2 className="w-4 h-4 shrink-0 mt-0.5 text-emerald-600 dark:text-emerald-400" />
                    ) : (
                      <AlertCircle className="w-4 h-4 shrink-0 mt-0.5 text-rose-600 dark:text-rose-400" />
                    )}
                    <div className="flex flex-col gap-0.5">
                      <span className="font-semibold">
                        {testFeedback.type === 'success' ? '测试连接成功' : '测试连接失败'}
                      </span>
                      <span className="text-[11px] leading-relaxed break-all select-text">
                        {testFeedback.message}
                      </span>
                    </div>
                  </div>
                )}
              </div>
            )}
          </div>
        </div>

        {/* 2. Vector Embedding & Semantic Search Configuration */}
        <div className="space-y-4">
          <h2 className="text-sm font-semibold uppercase tracking-wider text-slate-400">
            向量嵌入与语义检索
          </h2>
          <EmbeddingSettingsCard />
        </div>

        {/* 3. WebDAV connection settings (bidirectional sync is fail-closed until implemented) */}
        <div className="space-y-4">
          <h2 className="text-sm font-semibold uppercase tracking-wider text-slate-400">
            WebDAV 连接与凭据
          </h2>
          <WebdavSyncCard />
        </div>

        {/* 4. Physical Storage & Local Paths */}
        <div className="space-y-4">
          <h2 className="text-sm font-semibold uppercase tracking-wider text-slate-400">
            存储管理与物理目录
          </h2>

          <div className="bg-white dark:bg-slate-900 p-5 rounded-2xl border border-slate-200 dark:border-slate-800 shadow-xs space-y-3">
            <h3 className="text-sm font-semibold text-slate-900 dark:text-slate-100">关闭窗口时</h3>
            <p className="text-xs text-slate-500 dark:text-slate-400">
              点标题栏关闭默认会询问。最小化到托盘后，后台仍会监视浏览器历史。
            </p>
            <div className="flex flex-wrap gap-2">
              {([
                ['ask', '每次询问'],
                ['tray', '最小化到托盘'],
                ['quit', '退出程序'],
              ] as const).map(([value, label]) => (
                <button
                  key={value}
                  type="button"
                  onClick={async () => {
                    setCloseBehavior(value);
                    await tauriApi.setSetting('close_behavior', value);
                    showToast('关闭行为已保存', 'success');
                  }}
                  className={`px-3 py-1.5 rounded-xl text-xs border transition ${
                    closeBehavior === value
                      ? 'border-blue-500 bg-blue-50 text-blue-700 dark:bg-blue-950/40 dark:text-blue-300'
                      : 'border-slate-200 dark:border-slate-700 text-slate-600 dark:text-slate-300'
                  }`}
                >
                  {label}
                </button>
              ))}
            </div>
          </div>

          <StorageSettingsCard />

          {/* Local Storage & Paths Card */}
          {appInfo && (
            <div className="bg-white dark:bg-slate-900 p-5 rounded-2xl border border-slate-200 dark:border-slate-800 shadow-xs space-y-4">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-3">
                  <div className="p-2.5 rounded-xl bg-blue-500/10 text-blue-600 dark:text-blue-400 border border-blue-500/20">
                    <FolderTree className="w-5 h-5" />
                  </div>
                  <div>
                    <h3 className="text-sm font-semibold text-slate-900 dark:text-slate-100">
                      存储与诊断路径定位
                      {appInfo.is_portable && (
                        <span className="ml-2 text-[10px] px-1.5 py-0.5 rounded-full bg-amber-50 text-amber-700 dark:bg-amber-950/50 dark:text-amber-300 border border-amber-200 dark:border-amber-800">
                          便携模式
                        </span>
                      )}
                    </h3>
                    <p className="text-xs text-slate-500 dark:text-slate-400 mt-0.5">
                      {appInfo.is_portable
                        ? '数据写在程序旁 data 目录。卸载时请自行删除该目录。'
                        : '安装版数据在用户 AppData。卸载通常不会删除此目录。'}
                    </p>
                  </div>
                </div>
              </div>

              <div className="flex flex-col gap-2.5 font-mono text-xs">
                <div
                  onClick={() => {
                    tauriApi.openPathInFolder(appInfo.db_path).catch((e) => {
                      showToast(typeof e === 'string' ? e : '无法打开路径', 'error');
                    });
                  }}
                  className="flex items-center justify-between p-3 bg-slate-50/80 dark:bg-slate-850/60 rounded-xl border border-slate-100 dark:border-slate-800 hover:border-blue-300 dark:hover:border-blue-700 hover:bg-blue-50/40 dark:hover:bg-blue-950/20 transition cursor-pointer group"
                  title="点击在资源管理器中定位数据库文件"
                >
                  <div className="flex flex-col min-w-0 pr-3">
                    <span className="text-slate-500 dark:text-slate-400 font-sans text-xs group-hover:text-blue-600 dark:group-hover:text-blue-400 transition-colors">
                      {t('settings.databasePath')}
                    </span>
                    <span className="text-slate-800 dark:text-slate-200 break-all select-text font-mono mt-0.5 text-[11px]">
                      {appInfo.db_path}
                    </span>
                    <span className="text-slate-400 text-[10px] mt-0.5 font-sans">
                      数据库文件体积: {formatBytes(appInfo.db_size_bytes)}
                    </span>
                  </div>
                  <div className="p-2 rounded-lg text-slate-400 group-hover:text-blue-600 group-hover:bg-blue-100/60 dark:group-hover:bg-blue-900/40 transition shrink-0">
                    <FolderOpen className="w-4 h-4" />
                  </div>
                </div>

                <div
                  onClick={() => {
                    tauriApi.openPathInFolder(appInfo.temp_dir).catch((e) => {
                      showToast(typeof e === 'string' ? e : '无法打开路径', 'error');
                    });
                  }}
                  className="flex items-center justify-between p-3 bg-slate-50/80 dark:bg-slate-850/60 rounded-xl border border-slate-100 dark:border-slate-800 hover:border-blue-300 dark:hover:border-blue-700 hover:bg-blue-50/40 dark:hover:bg-blue-950/20 transition cursor-pointer group"
                  title="点击在资源管理器中打开临时目录"
                >
                  <div className="flex flex-col min-w-0 pr-3">
                    <span className="text-slate-500 dark:text-slate-400 font-sans text-xs group-hover:text-blue-600 dark:group-hover:text-blue-400 transition-colors">
                      {t('settings.tempPath')}
                    </span>
                    <span className="text-slate-800 dark:text-slate-200 break-all select-text font-mono mt-0.5 text-[11px]">
                      {appInfo.temp_dir}
                    </span>
                  </div>
                  <div className="p-2 rounded-lg text-slate-400 group-hover:text-blue-600 group-hover:bg-blue-100/60 dark:group-hover:bg-blue-900/40 transition shrink-0">
                    <FolderOpen className="w-4 h-4" />
                  </div>
                </div>

                <div
                  onClick={() => {
                    tauriApi.openPathInFolder(appInfo.logs_dir).catch((e) => {
                      showToast(typeof e === 'string' ? e : '无法打开路径', 'error');
                    });
                  }}
                  className="flex items-center justify-between p-3 bg-slate-50/80 dark:bg-slate-850/60 rounded-xl border border-slate-100 dark:border-slate-800 hover:border-blue-300 dark:hover:border-blue-700 hover:bg-blue-50/40 dark:hover:bg-blue-950/20 transition cursor-pointer group"
                  title="点击在资源管理器中打开日志目录"
                >
                  <div className="flex flex-col min-w-0 pr-3">
                    <span className="text-slate-500 dark:text-slate-400 font-sans text-xs group-hover:text-blue-600 dark:group-hover:text-blue-400 transition-colors">
                      {t('settings.logsPath')}
                    </span>
                    <span className="text-slate-800 dark:text-slate-200 break-all select-text font-mono mt-0.5 text-[11px]">
                      {appInfo.logs_dir}
                    </span>
                  </div>
                  <div className="p-2 rounded-lg text-slate-400 group-hover:text-blue-600 group-hover:bg-blue-100/60 dark:group-hover:bg-blue-900/40 transition shrink-0">
                    <FolderOpen className="w-4 h-4" />
                  </div>
                </div>
              </div>
            </div>
          )}
        </div>

        {/* 5. System Health Diagnostics */}
        <div className="space-y-4">
          <h2 className="text-sm font-semibold uppercase tracking-wider text-slate-400">
            系统自检与运行诊断
          </h2>
          <DiagnosticsCard />
        </div>

        {/* 6. Commercial License */}
        <div className="space-y-4">
          <h2 className="text-sm font-semibold uppercase tracking-wider text-slate-400">
            专业版授权与商业支持
          </h2>
          <LicenseSettingsCard />
        </div>

        {/* 7. About Browsory - Placed at the very bottom */}
        <div className="space-y-4">
          <h2 className="text-sm font-semibold uppercase tracking-wider text-slate-400">
            关于软件
          </h2>
          <AboutCard />
        </div>
      </div>
    </div>
  );
};
