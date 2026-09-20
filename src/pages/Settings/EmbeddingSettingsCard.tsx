import React, { useState, useEffect } from 'react';
import {
  Brain,
  ChevronDown,
  ChevronUp,
  Sparkles,
  Loader2,
  CheckCircle2,
  AlertCircle,
  ArrowRight,
  Layers,
} from 'lucide-react';
import { tauriApi } from '../../services/tauri';
import { useAppStore } from '../../stores/useAppStore';
import { EmbeddingIndexingStatus } from '../../types';
import { PresetManager, PresetItem } from './PresetManager';

export const EmbeddingSettingsCard: React.FC = () => {
  const { showToast, setCurrentTab } = useAppStore();

  const [isExpanded, setIsExpanded] = useState(true);
  const [embeddingModel, setEmbeddingModel] = useState('text-embedding-3-small');
  const [embeddingBaseUrl, setEmbeddingBaseUrl] = useState('');
  const [embeddingApiKey, setEmbeddingApiKey] = useState('');

  const [embeddingPresets, setEmbeddingPresets] = useState<PresetItem[]>([]);

  const [isTesting, setIsTesting] = useState(false);
  const [testFeedback, setTestFeedback] = useState<{
    type: 'success' | 'error';
    message: string;
  } | null>(null);

  const [indexingStatus, setIndexingStatus] = useState<EmbeddingIndexingStatus | null>(null);
  const [isBatchGenerating, setIsBatchGenerating] = useState(false);

  const loadSettingsAndStatus = async () => {
    try {
      const [savedModel, savedBaseUrl, savedKey, savedPresets] = await Promise.all([
        tauriApi.getSetting('ai_embedding_model'),
        tauriApi.getSetting('ai_embedding_base_url'),
        tauriApi.getSetting('ai_embedding_key'),
        tauriApi.getSetting('ai_presets_embedding'),
      ]);

      if (savedModel) setEmbeddingModel(savedModel);
      if (savedBaseUrl) setEmbeddingBaseUrl(savedBaseUrl);
      if (savedKey) setEmbeddingApiKey(savedKey);

      if (savedPresets) {
        try {
          const parsed = JSON.parse(savedPresets);
          if (Array.isArray(parsed)) {
            setEmbeddingPresets(parsed.slice(0, 3));
          }
        } catch {
          // ignore corrupted JSON
        }
      }

      const status = await tauriApi.getEmbeddingStatus(savedModel || undefined);
      setIndexingStatus(status);
    } catch (e) {
      console.warn('Failed to load embedding settings:', e);
    }
  };

  useEffect(() => {
    loadSettingsAndStatus();
  }, []);

  const handleSave = async () => {
    try {
      await tauriApi.setSetting('ai_embedding_model', embeddingModel.trim());
      await tauriApi.setSetting('ai_embedding_base_url', embeddingBaseUrl.trim());
      await tauriApi.setSetting('ai_embedding_key', embeddingApiKey.trim());

      showToast('向量嵌入配置已保存', 'success');
      const status = await tauriApi.getEmbeddingStatus(embeddingModel.trim());
      setIndexingStatus(status);
    } catch (err: any) {
      showToast(err?.message || '保存向量配置失败', 'error');
    }
  };

  const handleTestConnection = async () => {
    setIsTesting(true);
    setTestFeedback(null);
    try {
      const res = await tauriApi.testEmbeddingConnection({
        base_url: embeddingBaseUrl.trim() || undefined,
        api_key: embeddingApiKey.trim() || undefined,
        model: embeddingModel.trim(),
      });
      setTestFeedback({
        type: res.success ? 'success' : 'error',
        message: res.message,
      });
      if (res.success) {
        showToast('向量端点连接测试成功！', 'success');
      }
    } catch (err: any) {
      const msg = typeof err === 'string' ? err : err?.message || '连接失败，无法访问向量服务';
      setTestFeedback({
        type: 'error',
        message: msg,
      });
      showToast('向量接口测试失败', 'error');
    } finally {
      setIsTesting(false);
    }
  };

  const handleTriggerIndexBatch = async () => {
    setIsBatchGenerating(true);
    try {
      const count = await tauriApi.generateEmbeddingsBatch(embeddingModel.trim(), 30);
      showToast(`成功生成并增量保存 ${count} 个页面的向量索引`, 'success');
      const status = await tauriApi.getEmbeddingStatus(embeddingModel.trim());
      setIndexingStatus(status);
    } catch (err: any) {
      showToast(err?.message || '生成向量索引失败，请检查配置', 'error');
    } finally {
      setIsBatchGenerating(false);
    }
  };

  // Preset operations
  const handleApplyPreset = (preset: PresetItem) => {
    if (preset.model) setEmbeddingModel(preset.model);
    if (preset.baseUrl !== undefined) setEmbeddingBaseUrl(preset.baseUrl);
    if (preset.apiKey !== undefined) setEmbeddingApiKey(preset.apiKey);
    setTestFeedback(null);
  };

  const handleAddPreset = async (preset: PresetItem) => {
    if (embeddingPresets.length >= 3) {
      showToast('向量预设最多允许添加 3 个', 'warning');
      return;
    }
    const next = [...embeddingPresets, preset].slice(0, 3);
    setEmbeddingPresets(next);
    await tauriApi.setSetting('ai_presets_embedding', JSON.stringify(next));
    showToast(`已保存向量预设「${preset.name}」`, 'success');
  };

  const handleDeletePreset = async (index: number) => {
    const next = embeddingPresets.filter((_, i) => i !== index);
    setEmbeddingPresets(next);
    await tauriApi.setSetting('ai_presets_embedding', JSON.stringify(next));
    showToast('预设已删除', 'info');
  };

  const indexedCount = indexingStatus?.indexed_urls ?? 0;
  const unindexedCount = indexingStatus?.pending_urls ?? 0;
  const totalCount = indexingStatus?.total_urls ?? (indexedCount + unindexedCount);
  const percent = totalCount > 0 ? Math.round((indexedCount / totalCount) * 100) : 0;

  return (
    <div className="bg-white dark:bg-slate-900 rounded-2xl border border-slate-200 dark:border-slate-800 shadow-xs overflow-hidden transition-all">
      {/* Collapsible Header */}
      <button
        type="button"
        onClick={() => setIsExpanded(!isExpanded)}
        className="w-full px-4 py-3 flex items-center justify-between bg-slate-50/50 dark:bg-slate-800/50 hover:bg-slate-100/70 dark:hover:bg-slate-750 transition-colors cursor-pointer text-left"
      >
        <div className="flex items-center gap-2.5">
          <Brain className="w-4 h-4 text-emerald-500 shrink-0" />
          <div className="flex flex-col">
            <div className="flex items-center gap-2">
              <span className="font-semibold text-slate-800 dark:text-slate-100">
                向量嵌入与语义检索配置
              </span>
              <span className="text-[10px] px-1.5 py-0.5 rounded bg-emerald-50 text-emerald-700 dark:bg-emerald-950/60 dark:text-emerald-300 font-mono">
                {embeddingModel}
              </span>
            </div>
            <span className="text-[11px] text-slate-500 font-mono mt-0.5 truncate max-w-sm">
              驱动混合搜索与相似网页推荐 · 已索引 {indexedCount}/{totalCount} 页 ({percent}%)
            </span>
          </div>
        </div>

        <div className="flex items-center gap-2 text-slate-400">
          <span className="text-[11px]">{isExpanded ? '收起' : '展开配置'}</span>
          {isExpanded ? <ChevronUp className="w-4 h-4" /> : <ChevronDown className="w-4 h-4" />}
        </div>
      </button>

      {/* Expanded Body */}
      {isExpanded && (
        <div className="p-4 border-t border-slate-200 dark:border-slate-700/80 flex flex-col gap-4 animate-in fade-in-50 duration-150">
          {/* Embedding Status Progress Banner */}
          <div className="p-3 rounded-lg bg-slate-50 dark:bg-slate-900/60 border border-slate-200/80 dark:border-slate-800 flex flex-col gap-2">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <Layers className="w-4 h-4 text-blue-500 shrink-0" />
                <span className="font-medium text-slate-700 dark:text-slate-200 text-xs">
                  本地向量库覆盖状态
                </span>
                <span className="text-[11px] text-slate-400 font-mono">
                  (模型: {embeddingModel})
                </span>
              </div>
              <div className="flex items-center gap-2">
                <span className="font-semibold text-xs font-mono text-blue-600 dark:text-blue-400">
                  {percent}%
                </span>
                <span className="text-[11px] text-slate-400">
                  ({indexedCount} / {totalCount} 条)
                </span>
              </div>
            </div>

            {/* Progress Bar */}
            <div className="w-full h-1.5 bg-slate-200 dark:bg-slate-800 rounded-full overflow-hidden">
              <div
                className="h-full bg-emerald-500 rounded-full transition-all duration-300"
                style={{ width: `${percent}%` }}
              />
            </div>

            <div className="flex items-center justify-between pt-1 text-[11px] text-slate-500">
              <span>
                待向量化页面: <strong className="text-slate-700 dark:text-slate-300 font-mono">{unindexedCount}</strong> 个
              </span>
              <div className="flex items-center gap-2">
                {unindexedCount > 0 && (
                  <button
                    type="button"
                    onClick={handleTriggerIndexBatch}
                    disabled={isBatchGenerating}
                    className="flex items-center gap-1 text-blue-600 dark:text-blue-400 hover:underline cursor-pointer disabled:opacity-50"
                  >
                    {isBatchGenerating && <Loader2 className="w-3 h-3 animate-spin" />}
                    <span>增量索引 30 条</span>
                  </button>
                )}
                <button
                  type="button"
                  onClick={() => setCurrentTab('ai')}
                  className="flex items-center gap-1 text-slate-500 hover:text-slate-700 dark:hover:text-slate-300 cursor-pointer"
                >
                  <span>前往 AI 助手</span>
                  <ArrowRight className="w-3 h-3" />
                </button>
              </div>
            </div>
          </div>

          {/* Quick Presets (Default empty, max 3) */}
          <PresetManager
            label="向量预设:"
            presets={embeddingPresets}
            onApply={handleApplyPreset}
            onAdd={handleAddPreset}
            onDelete={handleDeletePreset}
            currentModel={embeddingModel}
            currentBaseUrl={embeddingBaseUrl}
            currentApiKey={embeddingApiKey}
            maxCount={3}
          />

          {/* Model Name Input */}
          <div className="flex flex-col gap-1">
            <label className="text-slate-600 dark:text-slate-400 flex items-center justify-between">
              <span className="font-medium">向量模型名称 (Embedding Model)</span>
              <span className="text-[10px] text-slate-400 font-mono">用于计算文本余弦相似度的模型标识符</span>
            </label>
            <input
              type="text"
              value={embeddingModel}
              onChange={(e) => setEmbeddingModel(e.target.value)}
              placeholder="例如 text-embedding-3-small、nomic-embed-text、bge-m3"
              className="px-2.5 py-1.5 rounded border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-900 text-slate-800 dark:text-slate-200 font-mono text-xs focus:outline-none focus:border-emerald-500"
            />
          </div>

          {/* Independent Base URL & API Key */}
          <div className="flex flex-col gap-3 p-3 rounded-lg border border-slate-200 dark:border-slate-700/80 bg-slate-50/50 dark:bg-slate-900/30">
            <div className="flex flex-col gap-1">
              <label className="text-slate-600 dark:text-slate-400 flex items-center justify-between">
                <span className="font-medium">向量服务 Base URL</span>
                <span className="text-[10px] text-slate-400 font-mono">标准 /embeddings 端点</span>
              </label>
              <input
                type="text"
                value={embeddingBaseUrl}
                onChange={(e) => setEmbeddingBaseUrl(e.target.value)}
                placeholder="例如 https://api.openai.com/v1 或 http://localhost:11434/v1"
                className="px-2.5 py-1.5 rounded border border-slate-300 dark:border-slate-700 bg-white dark:bg-slate-900 text-slate-800 dark:text-slate-200 font-mono text-xs focus:outline-none focus:border-emerald-500"
              />
            </div>

            <div className="flex flex-col gap-1">
              <label className="text-slate-600 dark:text-slate-400 flex items-center justify-between">
                <span className="font-medium">向量服务 API Key</span>
                <span className="text-[10px] text-slate-400 font-mono">本地 Ollama/LM Studio 等可留空</span>
              </label>
              <input
                type="password"
                value={embeddingApiKey}
                onChange={(e) => setEmbeddingApiKey(e.target.value)}
                placeholder="sk-...（本地模型留空）"
                className="px-2.5 py-1.5 rounded border border-slate-300 dark:border-slate-700 bg-white dark:bg-slate-900 text-slate-800 dark:text-slate-200 font-mono text-xs focus:outline-none focus:border-emerald-500"
              />
            </div>
          </div>

          {/* Actions & Test Button */}
          <div className="flex items-center gap-3 pt-1">
            <button
              type="button"
              onClick={handleSave}
              className="px-3.5 py-1.5 bg-emerald-600 hover:bg-emerald-700 text-white rounded font-medium transition shadow-sm cursor-pointer"
            >
              保存向量配置
            </button>

            <button
              type="button"
              onClick={handleTestConnection}
              disabled={isTesting}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded border border-slate-300 dark:border-slate-700 hover:bg-slate-100 dark:hover:bg-slate-700 text-slate-700 dark:text-slate-200 transition disabled:opacity-50 cursor-pointer"
            >
              {isTesting ? (
                <Loader2 className="w-3.5 h-3.5 animate-spin" />
              ) : (
                <Sparkles className="w-3.5 h-3.5 text-emerald-500" />
              )}
              <span>{isTesting ? '正在测试向量接口...' : '测试向量连接'}</span>
            </button>
          </div>

          {/* Test Feedback */}
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
                  {testFeedback.type === 'success' ? '向量端点测试通过' : '向量端点测试失败'}
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
  );
};
