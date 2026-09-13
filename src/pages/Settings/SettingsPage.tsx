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
} from 'lucide-react';
import { useAppStore } from '../../stores/useAppStore';
import { tauriApi } from '../../services/tauri';
import { AppInfo } from '../../types';
import { SecuritySettingsCard } from './SecuritySettingsCard';
import { MaintenanceSettingsCard } from './MaintenanceSettingsCard';
import { AboutCard } from './AboutCard';

export const SettingsPage: React.FC = () => {
  const { language, setLanguage, theme, setTheme, showToast, t } = useAppStore();
  const [appInfo, setAppInfo] = useState<AppInfo | null>(null);

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

  const [isTesting, setIsTesting] = useState(false);
  const [testFeedback, setTestFeedback] = useState<{
    type: 'success' | 'error';
    message: string;
  } | null>(null);

  useEffect(() => {
    tauriApi.getAppInfo().then(setAppInfo).catch(console.error);
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

  const applyPreset = (url: string, model: string) => {
    setAiBaseUrl(url);
    setAiModel(model);
    setTestFeedback(null);
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
    <div className="flex-1 flex flex-col h-full bg-slate-50 dark:bg-slate-900 overflow-y-auto p-6 text-xs select-none">
      <div className="pb-4 border-b border-slate-200 dark:border-slate-800 flex items-center justify-between">
        <h1 className="text-base font-semibold text-slate-800 dark:text-slate-100">
          {t('settings.title')}
        </h1>
        <div className="flex items-center gap-2">
          {/* 深浅色模式切换按钮 */}
          <button
            type="button"
            onClick={toggleTheme}
            className="flex items-center gap-1.5 px-2.5 py-1 rounded-md border border-slate-200 dark:border-slate-700 bg-white dark:bg-slate-800 hover:bg-slate-100 dark:hover:bg-slate-700/80 text-slate-700 dark:text-slate-200 text-xs transition shadow-xs cursor-pointer"
            title={isCurrentDark ? '切换为浅色模式' : '切换为深色模式'}
          >
            {isCurrentDark ? (
              <Sun className="w-3.5 h-3.5 text-amber-500" />
            ) : (
              <Moon className="w-3.5 h-3.5 text-slate-600" />
            )}
            <span className="font-medium">{isCurrentDark ? '深色模式' : '浅色模式'}</span>
          </button>

          {/* 界面语言切换按钮 */}
          <button
            type="button"
            onClick={() => setLanguage(language === 'zh-CN' ? 'en-US' : 'zh-CN')}
            className="flex items-center gap-1.5 px-2.5 py-1 rounded-md border border-slate-200 dark:border-slate-700 bg-white dark:bg-slate-800 hover:bg-slate-100 dark:hover:bg-slate-700/80 text-slate-700 dark:text-slate-200 text-xs transition shadow-xs cursor-pointer"
            title="切换界面语言 (Switch Language)"
          >
            <Languages className="w-3.5 h-3.5 text-blue-500" />
            <span className="font-medium">{language === 'zh-CN' ? '中 / EN' : 'EN / 中'}</span>
          </button>
        </div>
      </div>

      <div className="max-w-2xl flex flex-col gap-6 mt-5">
        {/* About Browsory & Release Notes */}
        <AboutCard />

        {/* Privacy & Security Card */}
        <SecuritySettingsCard />

        {/* AI API Configuration - Collapsible Card */}
        <div className="bg-white dark:bg-slate-800 rounded border border-slate-200 dark:border-slate-700 shadow-sm overflow-hidden transition-all">
          {/* Collapsible Header */}
          <button
            type="button"
            onClick={() => setIsAiExpanded(!isAiExpanded)}
            className="w-full px-4 py-3 flex items-center justify-between bg-slate-50/50 dark:bg-slate-800/50 hover:bg-slate-100/70 dark:hover:bg-slate-750 transition-colors cursor-pointer text-left"
          >
            <div className="flex items-center gap-2.5">
              <Sparkles className="w-4 h-4 text-purple-500 shrink-0" />
              <div className="flex flex-col">
                <div className="flex items-center gap-2">
                  <span className="font-semibold text-slate-800 dark:text-slate-100">
                    AI 智能分析接口配置
                  </span>
                  <span className="text-[10px] px-1.5 py-0.5 rounded bg-purple-50 text-purple-700 dark:bg-purple-950/60 dark:text-purple-300 font-mono">
                    {providerType === 'local' ? '本地模式' : '云端兼容'}
                  </span>
                </div>
                <span className="text-[11px] text-slate-500 font-mono mt-0.5 truncate max-w-sm">
                  {aiModel ? `${aiModel} @ ${aiBaseUrl}` : '未配置模型端点'}
                </span>
              </div>
            </div>

            <div className="flex items-center gap-2 text-slate-400">
              <span className="text-[11px]">{isAiExpanded ? '收起' : '展开配置'}</span>
              {isAiExpanded ? <ChevronUp className="w-4 h-4" /> : <ChevronDown className="w-4 h-4" />}
            </div>
          </button>

          {/* Collapsed Body */}
          {isAiExpanded && (
            <div className="p-4 border-t border-slate-200 dark:border-slate-700/80 flex flex-col gap-4 animate-in fade-in-50 duration-150">
              {/* Provider Mode Switcher */}
              <div className="flex flex-col gap-1.5">
                <label className="text-slate-600 dark:text-slate-400 font-medium">接口提供商模式</label>
                <div className="flex gap-2">
                  <button
                    type="button"
                    onClick={() => handleSelectProvider('cloud')}
                    className={`flex-1 py-2 px-3 rounded border text-left flex flex-col gap-0.5 transition ${
                      providerType === 'cloud'
                        ? 'border-purple-500 bg-purple-50/50 dark:bg-purple-950/30 text-purple-700 dark:text-purple-300 font-medium ring-1 ring-purple-500/20'
                        : 'border-slate-200 dark:border-slate-700 bg-slate-50 dark:bg-slate-900 text-slate-600 dark:text-slate-400 hover:bg-slate-100 dark:hover:bg-slate-800'
                    }`}
                  >
                    <span className="text-xs font-semibold">OpenAI 兼容 (云端接口)</span>
                    <span className="text-[10px] opacity-80">支持 OpenAI、DeepSeek、Moonshot 等</span>
                  </button>

                  <button
                    type="button"
                    onClick={() => handleSelectProvider('local')}
                    className={`flex-1 py-2 px-3 rounded border text-left flex flex-col gap-0.5 transition ${
                      providerType === 'local'
                        ? 'border-purple-500 bg-purple-50/50 dark:bg-purple-950/30 text-purple-700 dark:text-purple-300 font-medium ring-1 ring-purple-500/20'
                        : 'border-slate-200 dark:border-slate-700 bg-slate-50 dark:bg-slate-900 text-slate-600 dark:text-slate-400 hover:bg-slate-100 dark:hover:bg-slate-800'
                    }`}
                  >
                    <span className="text-xs font-semibold">本地模型 (Local LLM)</span>
                    <span className="text-[10px] opacity-80">支持本地运行的 Ollama、LM Studio 等</span>
                  </button>
                </div>
              </div>

              {/* Quick Presets */}
              <div className="flex items-center gap-2">
                <span className="text-slate-500 text-[11px]">快捷预设:</span>
                {providerType === 'cloud' ? (
                  <>
                    <button
                      type="button"
                      onClick={() => applyPreset('https://api.openai.com/v1', 'gpt-4o-mini')}
                      className="px-2 py-1 rounded bg-slate-100 dark:bg-slate-700 hover:bg-slate-200 dark:hover:bg-slate-600 text-slate-700 dark:text-slate-200 text-[11px] transition"
                    >
                      OpenAI (gpt-4o-mini)
                    </button>
                    <button
                      type="button"
                      onClick={() => applyPreset('https://api.deepseek.com', 'deepseek-chat')}
                      className="px-2 py-1 rounded bg-slate-100 dark:bg-slate-700 hover:bg-slate-200 dark:hover:bg-slate-600 text-slate-700 dark:text-slate-200 text-[11px] transition"
                    >
                      DeepSeek (deepseek-chat)
                    </button>
                  </>
                ) : (
                  <>
                    <button
                      type="button"
                      onClick={() => applyPreset('http://localhost:11434/v1', 'qwen2.5:7b')}
                      className="px-2 py-1 rounded bg-slate-100 dark:bg-slate-700 hover:bg-slate-200 dark:hover:bg-slate-600 text-slate-700 dark:text-slate-200 text-[11px] transition"
                    >
                      Ollama (qwen2.5:7b)
                    </button>
                    <button
                      type="button"
                      onClick={() => applyPreset('http://localhost:1234/v1', 'local-model')}
                      className="px-2 py-1 rounded bg-slate-100 dark:bg-slate-700 hover:bg-slate-200 dark:hover:bg-slate-600 text-slate-700 dark:text-slate-200 text-[11px] transition"
                    >
                      LM Studio (local-model)
                    </button>
                  </>
                )}
              </div>

              {/* Form Inputs */}
              <div className="flex flex-col gap-3">
                <div className="flex flex-col gap-1">
                  <label className="text-slate-600 dark:text-slate-400 flex items-center justify-between">
                    <span>Base URL</span>
                    <span className="text-[10px] text-slate-400">兼容 OpenAI 标准的 /chat/completions</span>
                  </label>
                  <input
                    type="text"
                    value={aiBaseUrl}
                    onChange={(e) => setAiBaseUrl(e.target.value)}
                    placeholder={providerType === 'local' ? 'http://localhost:11434/v1' : 'https://api.openai.com/v1'}
                    className="px-2.5 py-1.5 rounded border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-900 text-slate-800 dark:text-slate-200 font-mono"
                  />
                </div>

                <div className="flex flex-col gap-1">
                  <label className="text-slate-600 dark:text-slate-400 flex items-center justify-between">
                    <span>API Key {providerType === 'local' && <span className="text-emerald-600 dark:text-emerald-400 font-normal">(本地模型免 Key，可留空)</span>}</span>
                  </label>
                  <input
                    type="password"
                    value={aiKey}
                    onChange={(e) => setAiKey(e.target.value)}
                    placeholder={providerType === 'local' ? '本地模式无需 Key (可留空)' : 'sk-...'}
                    className="px-2.5 py-1.5 rounded border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-900 text-slate-800 dark:text-slate-200 font-mono"
                  />
                </div>

                <div className="flex flex-col gap-1">
                  <label className="text-slate-600 dark:text-slate-400">模型名称 (Model Name)</label>
                  <input
                    type="text"
                    value={aiModel}
                    onChange={(e) => setAiModel(e.target.value)}
                    placeholder={providerType === 'local' ? 'qwen2.5:7b 或 deepseek-r1' : 'gpt-4o-mini 或 deepseek-chat'}
                    className="px-2.5 py-1.5 rounded border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-900 text-slate-800 dark:text-slate-200 font-mono"
                  />
                </div>
              </div>

              {/* Action Buttons & Test Status */}
              <div className="flex items-center gap-3 pt-2">
                <button
                  type="button"
                  onClick={handleSaveAI}
                  className="px-3.5 py-1.5 bg-purple-600 hover:bg-purple-700 text-white rounded font-medium transition shadow-sm"
                >
                  保存 AI 配置
                </button>

                <button
                  type="button"
                  onClick={handleTestAI}
                  disabled={isTesting}
                  className="flex items-center gap-1.5 px-3 py-1.5 rounded border border-slate-300 dark:border-slate-700 hover:bg-slate-100 dark:hover:bg-slate-700 text-slate-700 dark:text-slate-200 transition disabled:opacity-50"
                >
                  {isTesting ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Sparkles className="w-3.5 h-3.5" />}
                  <span>{isTesting ? '正在测试...' : '测试连接'}</span>
                </button>
              </div>

              {/* Test Result Feedback Alert */}
              {testFeedback && (
                <div
                  className={`p-3 rounded text-xs flex items-start gap-2 border ${
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

        {/* Local Storage & Paths */}
        {appInfo && (
          <div className="bg-white dark:bg-slate-800 p-4 rounded border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-3">
            <div className="flex items-center justify-between">
              <span className="font-semibold text-slate-800 dark:text-slate-100">存储与诊断路径</span>
              <span className="text-[10px] text-slate-400">点击路径或右侧图标直接在资源管理器中打开</span>
            </div>

            <div className="flex flex-col gap-2 font-mono text-[11px]">
              <div
                onClick={() => {
                  tauriApi.openPathInFolder(appInfo.db_path).catch((e) => {
                    showToast(typeof e === 'string' ? e : '无法打开路径', 'error');
                  });
                }}
                className="flex items-center justify-between p-2.5 bg-slate-50 dark:bg-slate-900 rounded border border-slate-100 dark:border-slate-800/80 hover:border-blue-300 dark:hover:border-blue-700 hover:bg-blue-50/40 dark:hover:bg-blue-950/20 transition cursor-pointer group"
                title="点击在资源管理器中定位数据库文件"
              >
                <div className="flex flex-col min-w-0 pr-3">
                  <span className="text-slate-500 font-sans text-xs group-hover:text-blue-600 dark:group-hover:text-blue-400 transition-colors">
                    {t('settings.databasePath')}
                  </span>
                  <span className="text-slate-800 dark:text-slate-200 break-all select-text font-mono mt-0.5">
                    {appInfo.db_path}
                  </span>
                  <span className="text-slate-400 text-[10px] mt-0.5 font-sans">
                    占用体积: {formatBytes(appInfo.db_size_bytes)}
                  </span>
                </div>
                <div className="p-1.5 rounded text-slate-400 group-hover:text-blue-600 group-hover:bg-blue-100/60 dark:group-hover:bg-blue-900/40 transition shrink-0">
                  <FolderOpen className="w-4 h-4" />
                </div>
              </div>

              <div
                onClick={() => {
                  tauriApi.openPathInFolder(appInfo.temp_dir).catch((e) => {
                    showToast(typeof e === 'string' ? e : '无法打开路径', 'error');
                  });
                }}
                className="flex items-center justify-between p-2.5 bg-slate-50 dark:bg-slate-900 rounded border border-slate-100 dark:border-slate-800/80 hover:border-blue-300 dark:hover:border-blue-700 hover:bg-blue-50/40 dark:hover:bg-blue-950/20 transition cursor-pointer group"
                title="点击在资源管理器中打开临时目录"
              >
                <div className="flex flex-col min-w-0 pr-3">
                  <span className="text-slate-500 font-sans text-xs group-hover:text-blue-600 dark:group-hover:text-blue-400 transition-colors">
                    {t('settings.tempPath')}
                  </span>
                  <span className="text-slate-800 dark:text-slate-200 break-all select-text font-mono mt-0.5">
                    {appInfo.temp_dir}
                  </span>
                </div>
                <div className="p-1.5 rounded text-slate-400 group-hover:text-blue-600 group-hover:bg-blue-100/60 dark:group-hover:bg-blue-900/40 transition shrink-0">
                  <FolderOpen className="w-4 h-4" />
                </div>
              </div>

              <div
                onClick={() => {
                  tauriApi.openPathInFolder(appInfo.logs_dir).catch((e) => {
                    showToast(typeof e === 'string' ? e : '无法打开路径', 'error');
                  });
                }}
                className="flex items-center justify-between p-2.5 bg-slate-50 dark:bg-slate-900 rounded border border-slate-100 dark:border-slate-800/80 hover:border-blue-300 dark:hover:border-blue-700 hover:bg-blue-50/40 dark:hover:bg-blue-950/20 transition cursor-pointer group"
                title="点击在资源管理器中打开日志目录"
              >
                <div className="flex flex-col min-w-0 pr-3">
                  <span className="text-slate-500 font-sans text-xs group-hover:text-blue-600 dark:group-hover:text-blue-400 transition-colors">
                    {t('settings.logsPath')}
                  </span>
                  <span className="text-slate-800 dark:text-slate-200 break-all select-text font-mono mt-0.5">
                    {appInfo.logs_dir}
                  </span>
                </div>
                <div className="p-1.5 rounded text-slate-400 group-hover:text-blue-600 group-hover:bg-blue-100/60 dark:group-hover:bg-blue-900/40 transition shrink-0">
                  <FolderOpen className="w-4 h-4" />
                </div>
              </div>
            </div>
          </div>
        )}

        {/* Database Maintenance & Backups Card */}
        <MaintenanceSettingsCard />
      </div>
    </div>
  );
};
