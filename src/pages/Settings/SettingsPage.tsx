import React, { useEffect, useState, useRef } from 'react';
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
import { WebdavSyncCard } from './WebdavSyncCard';
import { AutoSyncSettingsCard } from './AutoSyncSettingsCard';
import { credentialRefForEndpoint, DEFAULT_AI_ENDPOINT } from '../../services/aiCredentials';
import { AiSettingsSaveError, saveAiSettings } from '../../services/saveAiSettings';
import { createRequestSequence } from '../../services/requestSequence';

export const SettingsPage: React.FC = () => {
  const { language, setLanguage, theme, setTheme, showToast, t } = useAppStore();
  const ui = (zh: string, en: string) => (language === 'en-US' ? en : zh);

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
  const credentialSequence = useRef(createRequestSequence());
  const [isCredentialLoading, setIsCredentialLoading] = useState(false);
  const [isSavingAI, setIsSavingAI] = useState(false);
  const [aiModel, setAiModel] = useState('gpt-4o-mini');

  const [cloudPresets, setCloudPresets] = useState<PresetItem[]>([]);
  const [localPresets, setLocalPresets] = useState<PresetItem[]>([]);

  const [isTesting, setIsTesting] = useState(false);
  const [testFeedback, setTestFeedback] = useState<{
    type: 'success' | 'error';
    message: string;
  } | null>(null);

  const migratePresets = async (
    settingKey: string,
    raw: string | null,
    scope: 'chat' | 'embedding',
  ): Promise<PresetItem[]> => {
    if (!raw) return [];
    let parsed: unknown;
    try {
      parsed = JSON.parse(raw);
    } catch {
      return [];
    }
    if (!Array.isArray(parsed)) return [];

    let changed = false;
    const migrated = await Promise.all(
      parsed.slice(0, 3).map(async (item): Promise<PresetItem | null> => {
        if (!item || typeof item !== 'object') return null;
        const legacy = item as Partial<PresetItem>;
        const baseUrl = (typeof legacy.baseUrl === 'string' ? legacy.baseUrl.trim() : '') || DEFAULT_AI_ENDPOINT;
        const credentialRef = await credentialRefForEndpoint(baseUrl, scope);
        const next: PresetItem = {
          id: typeof legacy.id === 'string' ? legacy.id : undefined,
          name: typeof legacy.name === 'string' ? legacy.name : 'Custom',
          model: typeof legacy.model === 'string' ? legacy.model : '',
          baseUrl,
          credentialRef,
        };
        if (legacy.credentialRef !== credentialRef || legacy.apiKey !== undefined) {
          changed = true;
        }

        if (typeof legacy.apiKey === 'string' && !legacy.apiKey.includes('•') && legacy.apiKey.trim()) {
          try {
            await tauriApi.setSetting(credentialRef, legacy.apiKey);
          } catch {
            next.apiKey = legacy.apiKey;
          }
        }
        return next;
      }),
    );
    const result = migrated.filter((item): item is PresetItem => item !== null);
    if (changed) {
      try {
        await tauriApi.setSetting(settingKey, JSON.stringify(result));
      } catch {
        // Retry from encrypted settings next time.
      }
    }
    return result;
  };

  useEffect(() => {
    tauriApi.getAppInfo().then(setAppInfo).catch(console.error);
    tauriApi.getSetting('close_behavior').then((v) => {
      if (v === 'tray' || v === 'quit' || v === 'ask') setCloseBehavior(v);
    });
    const initialRequest = credentialSequence.current.next();
    setIsCredentialLoading(true);
    tauriApi.getSetting('ai_base_url').then(async (v) => {
      if (!credentialSequence.current.isCurrent(initialRequest)) return;
      const baseUrl = v || 'https://api.openai.com/v1';
      if (v) {
        setAiBaseUrl(v);
        if (v.includes('localhost') || v.includes('127.0.0.1')) {
          setProviderType('local');
        } else {
          setProviderType('cloud');
        }
      }
      try {
        const key = (await tauriApi.getSetting(await credentialRefForEndpoint(baseUrl, 'chat'))) || '';
        if (credentialSequence.current.isCurrent(initialRequest)) setAiKey(key);
      } catch (err) {
        if (credentialSequence.current.isCurrent(initialRequest)) {
          showToast(typeof err === 'string' ? err : ui('读取 AI 凭据失败，请重试', 'Failed to load AI credentials; please retry'), 'error');
        }
      }
    }).catch((err) => {
      if (credentialSequence.current.isCurrent(initialRequest)) showToast(String(err), 'error');
    }).finally(() => {
      if (credentialSequence.current.isCurrent(initialRequest)) setIsCredentialLoading(false);
    });
    tauriApi.getSetting('ai_model').then((v) => v && setAiModel(v));

    tauriApi.getSetting('ai_presets_cloud').then(async (v) => setCloudPresets(await migratePresets('ai_presets_cloud', v, 'chat')));
    tauriApi.getSetting('ai_presets_local').then(async (v) => setLocalPresets(await migratePresets('ai_presets_local', v, 'chat')));
  }, []);

  const handleSelectProvider = (type: 'cloud' | 'local') => {
    credentialSequence.current.next();
    setIsCredentialLoading(false);
    setProviderType(type);
    setTestFeedback(null);
    if (type === 'local') {
      if (!aiBaseUrl.includes('localhost') && !aiBaseUrl.includes('127.0.0.1')) {
        setAiBaseUrl('http://localhost:11434/v1');
        setAiModel('qwen2.5:7b');
        setAiKey('');
      }
    } else {
      if (aiBaseUrl.includes('localhost') || aiBaseUrl.includes('127.0.0.1')) {
        setAiBaseUrl('https://api.openai.com/v1');
        setAiModel('gpt-4o-mini');
        setAiKey('');
      }
    }
  };

  const handleApplyPreset = async (preset: PresetItem) => {
    const request = credentialSequence.current.next();
    setIsCredentialLoading(true);
    if (preset.baseUrl) setAiBaseUrl(preset.baseUrl);
    if (preset.model) setAiModel(preset.model);
    setAiKey('');
    try {
      const key = (await tauriApi.getSetting(await credentialRefForEndpoint(preset.baseUrl, 'chat'))) || '';
      if (credentialSequence.current.isCurrent(request)) setAiKey(key);
    } catch (err) {
      if (credentialSequence.current.isCurrent(request)) {
        setAiKey('');
        showToast(typeof err === 'string' ? err : ui('读取预设凭据失败，请重试', 'Failed to load preset credentials; please retry'), 'error');
      }
    } finally {
      if (credentialSequence.current.isCurrent(request)) setIsCredentialLoading(false);
    }
    setTestFeedback(null);
  };

  const handleAddPreset = async (preset: PresetItem) => {
    if (providerType === 'cloud') {
      if (cloudPresets.length >= 3) {
        showToast(ui('OpenAI 兼容预设最多允许添加 3 个', 'Up to 3 OpenAI-compatible presets supported'), 'warning');
        return;
      }
      const next = [...cloudPresets, preset].slice(0, 3);
      if (aiKey && !aiKey.includes('•')) {
        const credentialRef = preset.credentialRef || await credentialRefForEndpoint(preset.baseUrl, 'chat');
        await tauriApi.setSetting(credentialRef, aiKey);
      }
      setCloudPresets(next);
      await tauriApi.setSetting('ai_presets_cloud', JSON.stringify(next));
      showToast(ui(`已保存预设「${preset.name}」`, `Saved preset "${preset.name}"`), 'success');
    } else {
      if (localPresets.length >= 3) {
        showToast(ui('本地模型预设最多允许添加 3 个', 'Up to 3 local presets supported'), 'warning');
        return;
      }
      const next = [...localPresets, preset].slice(0, 3);
      if (aiKey && !aiKey.includes('•')) {
        const credentialRef = preset.credentialRef || await credentialRefForEndpoint(preset.baseUrl, 'chat');
        await tauriApi.setSetting(credentialRef, aiKey);
      }
      setLocalPresets(next);
      await tauriApi.setSetting('ai_presets_local', JSON.stringify(next));
      showToast(ui(`已保存预设「${preset.name}」`, `Saved preset "${preset.name}"`), 'success');
    }
  };

  const handleDeletePreset = async (index: number) => {
    if (providerType === 'cloud') {
      const next = cloudPresets.filter((_, i) => i !== index);
      setCloudPresets(next);
      await tauriApi.setSetting('ai_presets_cloud', JSON.stringify(next));
      showToast(ui('预设已删除', 'Preset deleted'), 'info');
    } else {
      const next = localPresets.filter((_, i) => i !== index);
      setLocalPresets(next);
      await tauriApi.setSetting('ai_presets_local', JSON.stringify(next));
      showToast(ui('预设已删除', 'Preset deleted'), 'info');
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
        showToast(ui('AI 端点连接测试成功！', 'AI endpoint test passed!'), 'success');
      }
    } catch (err: any) {
      const msg = typeof err === 'string' ? err : err?.message || ui('连接失败，无法访问 AI 服务', 'Connection failed; AI service unreachable');
      setTestFeedback({
        type: 'error',
        message: msg,
      });
      showToast(ui('AI 端点连接测试失败', 'AI endpoint test failed'), 'error');
    } finally {
      setIsTesting(false);
    }
  };

  const handleSaveAI = async () => {
    if (isCredentialLoading || isSavingAI) return;
    setIsSavingAI(true);
    try {
      await saveAiSettings(tauriApi.setSetting, {
        credentialRef: await credentialRefForEndpoint(aiBaseUrl, 'chat'),
        apiKey: aiKey,
        baseUrl: aiBaseUrl.trim(),
        model: aiModel.trim(),
      });
      showToast(t('settings.aiSaved'), 'success');
    } catch (error) {
      const labels = {
        credential: t('settings.aiFieldCredential'),
        endpoint: t('settings.aiFieldEndpoint'),
        model: t('settings.aiFieldModel'),
      };
      const confirmed = error instanceof AiSettingsSaveError ? error.confirmedFields.map((field) => labels[field]).join(', ') : '';
      showToast(confirmed ? t('settings.aiPartlySaved', { fields: confirmed }) : t('settings.aiSaveUnconfirmed'), 'error');
    } finally {
      setIsSavingAI(false);
    }
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
      {/* Compact Desktop Header */}
      <div className="px-6 py-3.5 bg-white dark:bg-slate-900 border-b border-slate-200 dark:border-slate-800 shrink-0">
        <div className="max-w-6xl mx-auto flex flex-col sm:flex-row sm:items-center justify-between gap-3">
          <div className="flex items-center gap-3">
            <div className="p-2 rounded-xl bg-blue-600/10 text-blue-600 dark:text-blue-400 border border-blue-600/20">
              <SettingsIcon className="w-5 h-5" />
            </div>
            <div>
              <div className="flex items-center gap-2">
                <h1 className="text-base font-bold text-slate-900 dark:text-slate-100">
                  {t('settings.title')}
                </h1>
                <span className="px-2 py-0.5 text-[11px] font-medium rounded-full bg-blue-100 dark:bg-blue-950/80 text-blue-700 dark:text-blue-300 border border-blue-300 dark:border-blue-800">
                  {t('settings.systemLabel')}
                </span>
              </div>
              <p className="text-xs text-slate-500 dark:text-slate-400 mt-0.5">
                {t('settings.subtitle')}
              </p>
            </div>
          </div>

          <div className="flex items-center gap-2">
            {/* Theme toggle */}
            <button
              type="button"
              onClick={toggleTheme}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl border border-slate-200 dark:border-slate-700 bg-white dark:bg-slate-800 hover:bg-slate-100 dark:hover:bg-slate-700/80 text-slate-700 dark:text-slate-200 text-xs font-medium transition shadow-xs cursor-pointer"
              title={isCurrentDark ? t('settings.themeLight') : t('settings.themeDark')}
            >
              {isCurrentDark ? (
                <Sun className="w-3.5 h-3.5 text-amber-500" />
              ) : (
                <Moon className="w-3.5 h-3.5 text-slate-600" />
              )}
              <span>{isCurrentDark ? t('settings.themeLight') : t('settings.themeDark')}</span>
            </button>

            {/* Language toggle */}
            <button
              type="button"
              onClick={() => setLanguage(language === 'zh-CN' ? 'en-US' : 'zh-CN')}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl border border-slate-200 dark:border-slate-700 bg-white dark:bg-slate-800 hover:bg-slate-100 dark:hover:bg-slate-700/80 text-slate-700 dark:text-slate-200 text-xs font-medium transition shadow-xs cursor-pointer"
              title={t('settings.language')}
            >
              <Languages className="w-3.5 h-3.5 text-blue-500" />
              <span>{language === 'zh-CN' ? '中 / EN' : 'EN / 中'}</span>
            </button>
          </div>
        </div>
      </div>

      {/* Main Content Area */}
      <div className="max-w-6xl mx-auto w-full p-6 space-y-6">
        {/* 0. Auto History Sync Configuration */}
        <div className="space-y-3">
          <h2 className="text-xs font-semibold uppercase tracking-wider text-slate-400">
            {t('settings.autoSyncSection')}
          </h2>
          <AutoSyncSettingsCard />
        </div>

        {/* 1. AI API Configuration */}
        <div className="space-y-3">
          <h2 className="text-xs font-semibold uppercase tracking-wider text-slate-400">
            {t('settings.aiSection')}
          </h2>

          <div className="bg-white dark:bg-slate-900 rounded-2xl border border-slate-200 dark:border-slate-800 shadow-xs overflow-hidden transition-all">
            <button
              type="button"
              onClick={() => setIsAiExpanded(!isAiExpanded)}
              className="w-full px-5 py-4 flex items-center justify-between bg-slate-50/50 dark:bg-slate-800/50 hover:bg-slate-100/70 dark:hover:bg-slate-800 transition-colors cursor-pointer text-left"
            >
              <div className="flex items-center gap-3">
                <div className="p-2.5 rounded-xl bg-blue-500/10 text-blue-600 dark:text-blue-400 border border-blue-500/20">
                  <Sparkles className="w-5 h-5 shrink-0" />
                </div>
                <div className="flex flex-col">
                  <div className="flex items-center gap-2">
                    <span className="font-semibold text-slate-900 dark:text-slate-100 text-sm">
                      {ui('AI 大语言模型端点配置', 'AI chat model endpoint')}
                    </span>
                    <span className="text-[11px] px-2 py-0.5 rounded-full bg-blue-50 text-blue-700 dark:bg-blue-950/60 dark:text-blue-300 font-mono font-medium border border-blue-200 dark:border-blue-800">
                      {providerType === 'local' ? ui('本地模式', 'Local') : ui('云端兼容', 'Cloud')}
                    </span>
                  </div>
                  <span className="text-xs text-slate-500 dark:text-slate-400 font-mono mt-0.5 truncate max-w-md">
                    {aiModel ? `${aiModel} @ ${aiBaseUrl}` : ui('未配置模型端点', 'No model endpoint configured')}
                  </span>
                </div>
              </div>

              <div className="flex items-center gap-2 text-slate-400">
                <span className="text-xs">{isAiExpanded ? ui('收起配置', 'Collapse') : ui('展开配置', 'Expand')}</span>
                {isAiExpanded ? <ChevronUp className="w-4 h-4" /> : <ChevronDown className="w-4 h-4" />}
              </div>
            </button>

            {isAiExpanded && (
              <div className="p-5 border-t border-slate-100 dark:border-slate-800 flex flex-col gap-4 animate-in fade-in-50 duration-150">
                <div className="flex flex-col gap-1.5">
                  <label className="text-slate-600 dark:text-slate-300 text-xs font-medium">
                    {ui('接口提供商模式', 'Provider mode')}
                  </label>
                  <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                    <button
                      type="button"
                      onClick={() => handleSelectProvider('cloud')}
                      className={`p-3 rounded-xl border text-left flex flex-col gap-0.5 transition cursor-pointer ${
                        providerType === 'cloud'
                          ? 'border-blue-500 bg-blue-50/50 dark:bg-blue-950/30 text-blue-700 dark:text-blue-300 font-medium ring-1 ring-blue-500/20'
                          : 'border-slate-200 dark:border-slate-800 bg-slate-50 dark:bg-slate-900 text-slate-600 dark:text-slate-400 hover:bg-slate-100 dark:hover:bg-slate-800'
                      }`}
                    >
                      <span className="text-xs font-semibold">{ui('OpenAI 兼容 (云端接口)', 'OpenAI-compatible (Cloud)')}</span>
                      <span className="text-xs opacity-80">{ui('支持 OpenAI、DeepSeek、Moonshot 等', 'Supports OpenAI, DeepSeek, Moonshot, etc.')}</span>
                    </button>

                    <button
                      type="button"
                      onClick={() => handleSelectProvider('local')}
                      className={`p-3 rounded-xl border text-left flex flex-col gap-0.5 transition cursor-pointer ${
                        providerType === 'local'
                          ? 'border-blue-500 bg-blue-50/50 dark:bg-blue-950/30 text-blue-700 dark:text-blue-300 font-medium ring-1 ring-blue-500/20'
                          : 'border-slate-200 dark:border-slate-800 bg-slate-50 dark:bg-slate-900 text-slate-600 dark:text-slate-400 hover:bg-slate-100 dark:hover:bg-slate-800'
                      }`}
                    >
                      <span className="text-xs font-semibold">{ui('本地模型 (Local LLM)', 'Local LLM (Offline)')}</span>
                      <span className="text-xs opacity-80">{ui('支持本地运行的 Ollama、LM Studio 等离线引擎', 'Supports local Ollama, LM Studio, and OpenAI-compatible servers')}</span>
                    </button>
                  </div>
                </div>

                <PresetManager
                  label={providerType === 'cloud' ? ui('OpenAI 兼容预设:', 'Cloud presets:') : ui('本地模型预设:', 'Local presets:')}
                  presets={providerType === 'cloud' ? cloudPresets : localPresets}
                  onApply={handleApplyPreset}
                  onAdd={handleAddPreset}
                  onDelete={handleDeletePreset}
                  currentModel={aiModel}
                  currentBaseUrl={aiBaseUrl}
                  credentialScope="chat"
                  maxCount={3}
                />

                <div className="flex flex-col gap-3">
                  <div className="flex flex-col gap-1">
                    <label className="text-slate-600 dark:text-slate-300 text-xs flex items-center justify-between">
                      <span className="font-medium">Base URL</span>
                      <span className="text-[11px] text-slate-400">{ui('兼容 OpenAI 标准的 /chat/completions', 'OpenAI-compatible /chat/completions base URL')}</span>
                    </label>
                    <input
                      type="text"
                      value={aiBaseUrl}
                      onChange={(e) => {
                        credentialSequence.current.next();
                        setIsCredentialLoading(false);
                        setAiBaseUrl(e.target.value);
                        setAiKey('');
                      }}
                      placeholder={providerType === 'local' ? 'http://localhost:11434/v1' : 'https://api.openai.com/v1'}
                      className="px-3 py-2 text-xs rounded-xl border border-slate-200 dark:border-slate-800 bg-white dark:bg-slate-900 text-slate-800 dark:text-slate-200 font-mono focus:outline-hidden focus:ring-1 focus:ring-blue-500"
                    />
                  </div>

                  <div className="flex flex-col gap-1">
                    <label className="text-slate-600 dark:text-slate-300 text-xs flex items-center justify-between">
                      <span className="font-medium">
                        API Key{' '}
                        {providerType === 'local' && (
                          <span className="text-emerald-600 dark:text-emerald-400 font-normal">
                            {ui('(本地模型免 Key，可留空)', '(Optional for local endpoints)')}
                          </span>
                        )}
                      </span>
                    </label>
                    <input
                      type="password"
                      value={aiKey}
                      onChange={(e) => {
                        credentialSequence.current.next();
                        setIsCredentialLoading(false);
                        setAiKey(e.target.value);
                      }}
                      placeholder={providerType === 'local' ? ui('本地模式无需 Key (可留空)', 'Leave blank for local model') : 'sk-...'}
                      className="px-3 py-2 text-xs rounded-xl border border-slate-200 dark:border-slate-800 bg-white dark:bg-slate-900 text-slate-800 dark:text-slate-200 font-mono focus:outline-hidden focus:ring-1 focus:ring-blue-500"
                    />
                  </div>

                  <div className="flex flex-col gap-1">
                    <label className="text-slate-600 dark:text-slate-300 text-xs font-medium">{ui('模型名称 (Model Name)', 'Model name')}</label>
                    <input
                      type="text"
                      value={aiModel}
                      onChange={(e) => setAiModel(e.target.value)}
                      placeholder={providerType === 'local' ? ui('qwen2.5:7b 或 deepseek-r1', 'qwen2.5:7b or llama3.2') : 'gpt-4o-mini / deepseek-chat'}
                      className="px-3 py-2 text-xs rounded-xl border border-slate-200 dark:border-slate-800 bg-white dark:bg-slate-900 text-slate-800 dark:text-slate-200 font-mono focus:outline-hidden focus:ring-1 focus:ring-blue-500"
                    />
                  </div>
                </div>

                <div className="flex items-center gap-3 pt-2">
                  <button
                    type="button"
                    onClick={handleSaveAI}
                    disabled={isCredentialLoading || isSavingAI}
                    className="px-4 py-2 bg-blue-600 hover:bg-blue-700 text-white rounded-xl font-medium transition shadow-xs text-xs cursor-pointer disabled:opacity-50"
                  >
                    {ui('保存 AI 配置', 'Save AI settings')}
                  </button>

                  <button
                    type="button"
                    onClick={handleTestAI}
                    disabled={isTesting || isCredentialLoading || isSavingAI}
                    className="flex items-center gap-1.5 px-3.5 py-2 rounded-xl border border-slate-200 dark:border-slate-700 hover:bg-slate-100 dark:hover:bg-slate-800 text-slate-700 dark:text-slate-200 transition disabled:opacity-50 text-xs cursor-pointer"
                  >
                    {isTesting ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Sparkles className="w-3.5 h-3.5 text-blue-500" />}
                    <span>{isTesting ? ui('正在测试...', 'Testing...') : ui('测试连接', 'Test connection')}</span>
                  </button>
                </div>

                {testFeedback && (
                  <div
                    role="status"
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
                        {testFeedback.type === 'success' ? ui('测试连接成功', 'Connection test passed') : ui('测试连接失败', 'Connection test failed')}
                      </span>
                      <span className="text-xs leading-relaxed break-all select-text">
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
        <div className="space-y-3">
          <h2 className="text-xs font-semibold uppercase tracking-wider text-slate-400">
            {t('settings.embeddingSection')}
          </h2>
          <EmbeddingSettingsCard />
        </div>

        {/* 3. WebDAV connection settings */}
        <div className="space-y-3">
          <h2 className="text-xs font-semibold uppercase tracking-wider text-slate-400">
            {t('settings.webdavSection')}
          </h2>
          <WebdavSyncCard />
        </div>

        {/* 4. Physical Storage & Local Paths */}
        <div className="space-y-3">
          <h2 className="text-xs font-semibold uppercase tracking-wider text-slate-400">
            {t('settings.storageSection')}
          </h2>

          <div className="bg-white dark:bg-slate-900 p-5 rounded-2xl border border-slate-200 dark:border-slate-800 shadow-xs space-y-3">
            <h3 className="text-sm font-semibold text-slate-900 dark:text-slate-100">
              {ui('关闭窗口时', 'Window close behavior')}
            </h3>
            <p className="text-xs text-slate-500 dark:text-slate-400">
              {ui(
                '点标题栏关闭默认会询问。最小化到托盘后，后台仍会监视浏览器历史。',
                'Closing the main window asks by default. When minimized to tray, local snapshot monitoring stays active.'
              )}
            </p>
            <div className="flex flex-wrap gap-2">
              {([
                ['ask', ui('每次询问', 'Always ask')],
                ['tray', ui('最小化到托盘', 'Minimize to tray')],
                ['quit', ui('退出程序', 'Quit app')],
              ] as const).map(([value, label]) => (
                <button
                  key={value}
                  type="button"
                  onClick={async () => {
                    setCloseBehavior(value);
                    await tauriApi.setSetting('close_behavior', value);
                    showToast(ui('关闭行为已保存', 'Close behavior saved'), 'success');
                  }}
                  className={`px-3 py-1.5 rounded-xl text-xs border transition cursor-pointer ${
                    closeBehavior === value
                      ? 'border-blue-500 bg-blue-50 text-blue-700 dark:bg-blue-950/40 dark:text-blue-300 font-medium'
                      : 'border-slate-200 dark:border-slate-700 text-slate-600 dark:text-slate-300 hover:bg-slate-50 dark:hover:bg-slate-800'
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
                      {ui('存储与诊断路径定位', 'Local data & diagnostic paths')}
                      {appInfo.is_portable && (
                        <span className="ml-2 text-[11px] px-1.5 py-0.5 rounded-full bg-amber-50 text-amber-700 dark:bg-amber-950/50 dark:text-amber-300 border border-amber-200 dark:border-amber-800">
                          {ui('便携模式', 'Portable')}
                        </span>
                      )}
                    </h3>
                    <p className="text-xs text-slate-500 dark:text-slate-400 mt-0.5">
                      {appInfo.is_portable
                        ? ui('数据写在程序旁 data 目录。卸载时请自行删除该目录。', 'Data is stored next to the executable under data/. Delete that directory manually to uninstall cleanly.')
                        : ui('安装版数据在用户 AppData。卸载通常不会删除此目录。', 'Installed builds store vault data in AppData; uninstallers keep it intact unless removed manually.')}
                    </p>
                  </div>
                </div>
              </div>

              <div className="flex flex-col gap-2.5 font-mono text-xs">
                {[
                  {
                    path: appInfo.db_path,
                    label: t('settings.databasePath'),
                    title: ui('点击在资源管理器中定位数据库文件', 'Open database location in Explorer'),
                    extra: ui(`数据库文件体积: ${formatBytes(appInfo.db_size_bytes)}`, `Database size: ${formatBytes(appInfo.db_size_bytes)}`),
                  },
                  {
                    path: appInfo.temp_dir,
                    label: t('settings.tempPath'),
                    title: ui('点击在资源管理器中打开临时目录', 'Open temporary snapshot directory'),
                  },
                  {
                    path: appInfo.logs_dir,
                    label: t('settings.logsPath'),
                    title: ui('点击在资源管理器中打开日志目录', 'Open logs directory'),
                  },
                ].map((entry) => (
                  <button
                    key={entry.label}
                    type="button"
                    onClick={() => {
                      tauriApi.openPathInFolder(entry.path).catch((e) => {
                        showToast(typeof e === 'string' ? e : ui('无法打开路径', 'Could not open folder'), 'error');
                      });
                    }}
                    className="w-full text-left flex items-center justify-between p-3 bg-slate-50/80 dark:bg-slate-800/60 rounded-xl border border-slate-100 dark:border-slate-800 hover:border-blue-300 dark:hover:border-blue-700 hover:bg-blue-50/40 dark:hover:bg-blue-950/20 transition cursor-pointer group"
                    title={entry.title}
                  >
                    <div className="flex flex-col min-w-0 pr-3">
                      <span className="text-slate-500 dark:text-slate-400 font-sans text-xs group-hover:text-blue-600 dark:group-hover:text-blue-400 transition-colors">
                        {entry.label}
                      </span>
                      <span className="text-slate-800 dark:text-slate-200 break-all select-text font-mono mt-0.5 text-xs">
                        {entry.path}
                      </span>
                      {entry.extra && (
                        <span className="text-slate-400 text-[11px] mt-0.5 font-sans">
                          {entry.extra}
                        </span>
                      )}
                    </div>
                    <div className="p-2 rounded-lg text-slate-400 group-hover:text-blue-600 group-hover:bg-blue-100/60 dark:group-hover:bg-blue-900/40 transition shrink-0">
                      <FolderOpen className="w-4 h-4" />
                    </div>
                  </button>
                ))}
              </div>
            </div>
          )}
        </div>

        {/* 5. System Health Diagnostics */}
        <div className="space-y-3">
          <h2 className="text-xs font-semibold uppercase tracking-wider text-slate-400">
            {t('settings.diagnosticsSection')}
          </h2>
          <DiagnosticsCard />
        </div>

        {/* 6. About Browsory */}
        <div className="space-y-3">
          <h2 className="text-xs font-semibold uppercase tracking-wider text-slate-400">
            {t('settings.aboutSection')}
          </h2>
          <AboutCard />
        </div>
      </div>
    </div>
  );
};
