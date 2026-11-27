import React, { useState, useEffect, useRef } from 'react';
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
import { credentialRefForEndpoint, DEFAULT_AI_ENDPOINT } from '../../services/aiCredentials';
import { AiSettingsSaveError, saveAiSettings } from '../../services/saveAiSettings';
import { createRequestSequence } from '../../services/requestSequence';
import { useAppStore } from '../../stores/useAppStore';
import { EmbeddingIndexingStatus } from '../../types';
import { PresetManager, PresetItem } from './PresetManager';

export const EmbeddingSettingsCard: React.FC = () => {
  const { showToast, setCurrentTab, t, language } = useAppStore();
  const ui = (zh: string, en: string) => (language === 'en-US' ? en : zh);
  const credentialSequence = useRef(createRequestSequence());
  const [isCredentialLoading, setIsCredentialLoading] = useState(false);
  const [isSaving, setIsSaving] = useState(false);

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
    const request = credentialSequence.current.next();
    setIsCredentialLoading(true);
    try {
      const [savedModel, savedBaseUrl, savedPresets] = await Promise.all([
        tauriApi.getSetting('ai_embedding_model'),
        tauriApi.getSetting('ai_embedding_base_url'),
        tauriApi.getSetting('ai_presets_embedding'),
      ]);

      if (!credentialSequence.current.isCurrent(request)) return;
      if (savedModel) setEmbeddingModel(savedModel);
      if (savedBaseUrl) setEmbeddingBaseUrl(savedBaseUrl);

      const endpoint = savedBaseUrl?.trim() || DEFAULT_AI_ENDPOINT;
      const savedKey = await tauriApi.getSetting(await credentialRefForEndpoint(endpoint, 'embedding'));
      if (!credentialSequence.current.isCurrent(request)) return;
      setEmbeddingApiKey(savedKey || '');

      if (savedPresets) {
        try {
          const parsed = JSON.parse(savedPresets);
          if (Array.isArray(parsed)) {
            let changed = false;
            const migrated = await Promise.all(parsed.slice(0, 3).map(async (item: PresetItem) => {
              const baseUrl = (typeof item.baseUrl === 'string' ? item.baseUrl.trim() : '') || DEFAULT_AI_ENDPOINT;
              const credentialRef = await credentialRefForEndpoint(baseUrl, 'embedding');
              const next: PresetItem = {
                id: item.id,
                name: item.name,
                model: item.model,
                baseUrl,
                credentialRef,
              };
              if (item.credentialRef !== credentialRef || item.apiKey !== undefined) changed = true;
              if (typeof item.apiKey === 'string' && !item.apiKey.includes('•') && item.apiKey.trim()) {
                try {
                  await tauriApi.setSetting(credentialRef, item.apiKey);
                } catch {
                  next.apiKey = item.apiKey;
                }
              }
              return next;
            }));
            setEmbeddingPresets(migrated);
            if (changed) {
              await tauriApi.setSetting('ai_presets_embedding', JSON.stringify(migrated));
            }
          }
        } catch {
          // ignore corrupted JSON
        }
      }

      const status = await tauriApi.getEmbeddingStatus(savedModel || undefined);
      setIndexingStatus(status);
    } catch (e) {
      if (credentialSequence.current.isCurrent(request)) {
        showToast(typeof e === 'string' ? e : ui('读取向量配置或凭据失败，请重试', 'Failed to load embedding config or credentials; please retry'), 'error');
      }
    } finally {
      if (credentialSequence.current.isCurrent(request)) setIsCredentialLoading(false);
    }
  };

  useEffect(() => {
    loadSettingsAndStatus();
  }, []);

  const handleSave = async () => {
    if (isCredentialLoading || isSaving) return;
    setIsSaving(true);
    try {
      const endpoint = embeddingBaseUrl.trim() || DEFAULT_AI_ENDPOINT;
      await saveAiSettings(tauriApi.setSetting, {
        credentialRef: await credentialRefForEndpoint(endpoint, 'embedding'),
        apiKey: embeddingApiKey.trim(),
        baseUrl: endpoint,
        model: embeddingModel.trim(),
        embedding: true,
      });
      showToast(t('embedding.saved'), 'success');
      try {
        setIndexingStatus(await tauriApi.getEmbeddingStatus(embeddingModel.trim()));
      } catch {
        showToast(t('embedding.statusLoadFailed'), 'warning');
      }
    } catch (error) {
      const labels = {
        credential: t('settings.aiFieldCredential'),
        endpoint: t('settings.aiFieldEndpoint'),
        model: t('settings.aiFieldModel'),
      };
      const confirmed = error instanceof AiSettingsSaveError ? error.confirmedFields.map((field) => labels[field]).join(', ') : '';
      showToast(confirmed ? t('settings.aiPartlySaved', { fields: confirmed }) : t('settings.aiSaveUnconfirmed'), 'error');
    } finally {
      setIsSaving(false);
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
        showToast(t('embedding.connectionSuccess'), 'success');
      }
    } catch (err: any) {
      const msg = typeof err === 'string' ? err : err?.message || t('embedding.connectionFailed');
      setTestFeedback({
        type: 'error',
        message: msg,
      });
      showToast(t('embedding.testFailed'), 'error');
    } finally {
      setIsTesting(false);
    }
  };

  const handleTriggerIndexBatch = async () => {
    setIsBatchGenerating(true);
    try {
      const count = await tauriApi.generateEmbeddingsBatch(embeddingModel.trim(), 30);
      showToast(t('embedding.batchSuccess', { count }), 'success');
      const status = await tauriApi.getEmbeddingStatus(embeddingModel.trim());
      setIndexingStatus(status);
    } catch (err: any) {
      showToast(typeof err === 'string' ? err : err?.message || t('embedding.batchFailed'), 'error');
    } finally {
      setIsBatchGenerating(false);
    }
  };

  const handleApplyPreset = async (preset: PresetItem) => {
    const request = credentialSequence.current.next();
    setIsCredentialLoading(true);
    if (preset.model) setEmbeddingModel(preset.model);
    if (preset.baseUrl !== undefined) setEmbeddingBaseUrl(preset.baseUrl);
    setEmbeddingApiKey('');
    try {
      const key = (await tauriApi.getSetting(
        await credentialRefForEndpoint(preset.baseUrl.trim() || DEFAULT_AI_ENDPOINT, 'embedding'),
      )) || '';
      if (credentialSequence.current.isCurrent(request)) setEmbeddingApiKey(key);
    } catch (err: any) {
      if (credentialSequence.current.isCurrent(request)) {
        showToast(typeof err === 'string' ? err : ui('读取向量预设凭据失败，请重试', 'Failed to load preset credentials; please retry'), 'error');
      }
    } finally {
      if (credentialSequence.current.isCurrent(request)) setIsCredentialLoading(false);
    }
    setTestFeedback(null);
  };

  const handleAddPreset = async (preset: PresetItem) => {
    if (embeddingPresets.length >= 3) {
      showToast(t('embedding.maxPresets'), 'warning');
      return;
    }
    if (embeddingApiKey && !embeddingApiKey.includes('•')) {
      const credentialRef = preset.credentialRef || await credentialRefForEndpoint(preset.baseUrl, 'embedding');
      await tauriApi.setSetting(credentialRef, embeddingApiKey);
    }
    const next = [...embeddingPresets, preset].slice(0, 3);
    setEmbeddingPresets(next);
    await tauriApi.setSetting('ai_presets_embedding', JSON.stringify(next));
    showToast(t('embedding.presetSaved', { name: preset.name }), 'success');
  };

  const handleDeletePreset = async (index: number) => {
    const next = embeddingPresets.filter((_, i) => i !== index);
    setEmbeddingPresets(next);
    await tauriApi.setSetting('ai_presets_embedding', JSON.stringify(next));
    showToast(t('embedding.presetDeleted'), 'info');
  };

  const indexedCount = indexingStatus?.indexed_urls ?? 0;
  const unindexedCount = indexingStatus?.pending_urls ?? 0;
  const totalCount = indexingStatus?.total_urls ?? (indexedCount + unindexedCount);
  const percent = totalCount > 0 ? Math.min(100, Math.max(0, Math.round((indexedCount / totalCount) * 100))) : 0;

  return (
    <div className="bg-white dark:bg-slate-900 rounded-2xl border border-slate-200 dark:border-slate-800 shadow-xs overflow-hidden transition-all">
      {/* Collapsible Header */}
      <button
        type="button"
        onClick={() => setIsExpanded(!isExpanded)}
        className="w-full px-5 py-4 flex items-center justify-between bg-slate-50/50 dark:bg-slate-800/50 hover:bg-slate-100/70 dark:hover:bg-slate-700 transition-colors cursor-pointer text-left"
      >
        <div className="flex items-center gap-3">
          <div className="p-2 rounded-xl bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border border-emerald-500/20">
            <Brain className="w-5 h-5 shrink-0" />
          </div>
          <div className="flex flex-col">
            <div className="flex items-center gap-2">
              <span className="font-semibold text-sm text-slate-800 dark:text-slate-100">
                {t('embedding.title')}
              </span>
              <span className="text-[11px] px-2 py-0.5 rounded-full bg-emerald-50 text-emerald-700 dark:bg-emerald-950/60 dark:text-emerald-300 font-mono">
                {embeddingModel}
              </span>
            </div>
            <span className="text-xs text-slate-500 font-mono mt-0.5 truncate max-w-md">
              {t('embedding.summary', { indexed: indexedCount, total: totalCount, percent })}
            </span>
          </div>
        </div>

        <div className="flex items-center gap-2 text-slate-400">
          <span className="text-xs">{isExpanded ? t('embedding.collapse') : t('embedding.expand')}</span>
          {isExpanded ? <ChevronUp className="w-4 h-4" /> : <ChevronDown className="w-4 h-4" />}
        </div>
      </button>

      {/* Expanded Body */}
      {isExpanded && (
        <div className="p-5 border-t border-slate-200 dark:border-slate-700/80 flex flex-col gap-4 animate-in fade-in-50 duration-150">
          {/* Embedding Status Progress Banner */}
          <div className="p-3 rounded-lg bg-slate-50 dark:bg-slate-900/60 border border-slate-200/80 dark:border-slate-800 flex flex-col gap-2">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <Layers className="w-4 h-4 text-blue-500 shrink-0" />
                <span className="font-medium text-slate-700 dark:text-slate-200 text-xs">
                  {t('embedding.coverage')}
                </span>
                <span className="text-[11px] text-slate-400 font-mono">
                  ({ui(`模型: ${embeddingModel}`, `Model: ${embeddingModel}`)})
                </span>
              </div>
              <div className="flex items-center gap-2">
                <span className="font-semibold text-xs font-mono text-blue-600 dark:text-blue-400">
                  {percent}%
                </span>
                <span className="text-[11px] text-slate-400">
                  {t('embedding.records', { indexed: indexedCount, total: totalCount })}
                </span>
              </div>
            </div>

            <div className="w-full h-1.5 bg-slate-200 dark:bg-slate-800 rounded-full overflow-hidden">
              <div
                className="h-full bg-emerald-500 rounded-full transition-all duration-300"
                style={{ width: `${percent}%` }}
              />
            </div>

            <div className="flex items-center justify-between pt-1 text-xs text-slate-500">
              <span>
                {t('embedding.pending')} <strong className="text-slate-700 dark:text-slate-300 font-mono">{unindexedCount}</strong>
              </span>
              <div className="flex items-center gap-3">
                {unindexedCount > 0 && (
                  <button
                    type="button"
                    onClick={handleTriggerIndexBatch}
                    disabled={isBatchGenerating}
                    className="flex items-center gap-1 text-blue-600 dark:text-blue-400 hover:underline cursor-pointer disabled:opacity-50"
                  >
                    {isBatchGenerating && <Loader2 className="w-3 h-3 animate-spin" />}
                    <span>{t('embedding.incremental')}</span>
                  </button>
                )}
                <button
                  type="button"
                  onClick={() => setCurrentTab('ai')}
                  className="flex items-center gap-1 text-slate-500 hover:text-slate-700 dark:hover:text-slate-300 cursor-pointer"
                >
                  <span>{t('embedding.goAi')}</span>
                  <ArrowRight className="w-3 h-3" />
                </button>
              </div>
            </div>
          </div>

          <PresetManager
            label={t('embedding.presets')}
            presets={embeddingPresets}
            onApply={handleApplyPreset}
            onAdd={handleAddPreset}
            onDelete={handleDeletePreset}
            currentModel={embeddingModel}
            currentBaseUrl={embeddingBaseUrl}
            credentialScope="embedding"
            maxCount={3}
          />

          <div className="flex flex-col gap-1">
            <label className="text-slate-600 dark:text-slate-400 text-xs flex items-center justify-between">
              <span className="font-medium">{t('embedding.model')}</span>
              <span className="text-[11px] text-slate-400 font-mono">{t('embedding.modelHelp')}</span>
            </label>
            <input
              type="text"
              value={embeddingModel}
              onChange={(e) => setEmbeddingModel(e.target.value)}
              placeholder={t('embedding.modelPlaceholder')}
              className="px-3 py-2 rounded-xl border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-900 text-slate-800 dark:text-slate-200 font-mono text-xs focus:outline-none focus:border-blue-500"
            />
          </div>

          <div className="flex flex-col gap-3 p-3.5 rounded-xl border border-slate-200 dark:border-slate-700/80 bg-slate-50/50 dark:bg-slate-900/30">
            <div className="flex flex-col gap-1">
              <label className="text-slate-600 dark:text-slate-400 text-xs flex items-center justify-between">
                <span className="font-medium">{t('embedding.baseUrl')}</span>
                <span className="text-[11px] text-slate-400 font-mono">{t('embedding.baseUrlHelp')}</span>
              </label>
              <input
                type="text"
                value={embeddingBaseUrl}
                onChange={(e) => {
                  credentialSequence.current.next();
                  setIsCredentialLoading(false);
                  setEmbeddingBaseUrl(e.target.value);
                  setEmbeddingApiKey('');
                }}
                placeholder={t('embedding.baseUrlPlaceholder')}
                className="px-3 py-2 rounded-xl border border-slate-300 dark:border-slate-700 bg-white dark:bg-slate-900 text-slate-800 dark:text-slate-200 font-mono text-xs focus:outline-none focus:border-blue-500"
              />
            </div>

            <div className="flex flex-col gap-1">
              <label className="text-slate-600 dark:text-slate-400 text-xs flex items-center justify-between">
                <span className="font-medium">{t('embedding.apiKey')}</span>
                <span className="text-[11px] text-slate-400 font-mono">{t('embedding.apiKeyHelp')}</span>
              </label>
              <input
                type="password"
                value={embeddingApiKey}
                onChange={(e) => {
                  credentialSequence.current.next();
                  setIsCredentialLoading(false);
                  setEmbeddingApiKey(e.target.value);
                }}
                placeholder={t('embedding.apiKeyPlaceholder')}
                className="px-3 py-2 rounded-xl border border-slate-300 dark:border-slate-700 bg-white dark:bg-slate-900 text-slate-800 dark:text-slate-200 font-mono text-xs focus:outline-none focus:border-blue-500"
              />
            </div>
          </div>

          <div className="flex items-center gap-3 pt-1">
            <button
              type="button"
              onClick={handleSave}
              disabled={isCredentialLoading || isSaving}
              className="px-4 py-2 bg-blue-600 hover:bg-blue-700 text-white rounded-xl font-medium transition shadow-xs text-xs cursor-pointer disabled:opacity-50"
            >
              {t('embedding.save')}
            </button>

            <button
              type="button"
              onClick={handleTestConnection}
              disabled={isTesting || isCredentialLoading || isSaving}
              className="flex items-center gap-1.5 px-3.5 py-2 rounded-xl border border-slate-300 dark:border-slate-700 hover:bg-slate-100 dark:hover:bg-slate-700 text-slate-700 dark:text-slate-200 text-xs transition disabled:opacity-50 cursor-pointer"
            >
              {isTesting ? (
                <Loader2 className="w-3.5 h-3.5 animate-spin" />
              ) : (
                <Sparkles className="w-3.5 h-3.5 text-blue-500" />
              )}
              <span>{isTesting ? t('embedding.testing') : t('embedding.test')}</span>
            </button>
          </div>

          {testFeedback && (
            <div
              role="status"
              className={`p-3 rounded-xl text-xs flex items-start gap-2 border ${
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
                  {testFeedback.type === 'success' ? t('embedding.testSuccess') : t('embedding.testFailure')}
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
  );
};
