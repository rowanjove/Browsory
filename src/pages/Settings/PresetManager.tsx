import React, { useState } from 'react';
import { X, BookmarkPlus } from 'lucide-react';
import { credentialRefForEndpoint, DEFAULT_AI_ENDPOINT } from '../../services/aiCredentials';
import { useAppStore } from '../../stores/useAppStore';

export interface PresetItem {
  id?: string;
  name: string;
  model: string;
  baseUrl: string;
  /** Endpoint-bound OS credential reference, never the credential value. */
  credentialRef?: string;
  /** Legacy field accepted only for one-time migration; never written by this UI. */
  apiKey?: string;
}

interface PresetManagerProps {
  label?: string;
  presets: PresetItem[];
  onApply: (preset: PresetItem) => void;
  onAdd: (preset: PresetItem) => void;
  onDelete: (index: number) => void;
  currentModel: string;
  currentBaseUrl: string;
  credentialScope?: 'chat' | 'embedding';
  maxCount?: number;
}

export const PresetManager: React.FC<PresetManagerProps> = ({
  label,
  presets,
  onApply,
  onAdd,
  onDelete,
  currentModel,
  currentBaseUrl,
  credentialScope = label && (label.includes('向量') || label.toLowerCase().includes('embedding')) ? 'embedding' : 'chat',
  maxCount = 3,
}) => {
  const { language, t } = useAppStore();
  const ui = (zh: string, en: string) => (language === 'en-US' ? en : zh);
  const resolvedLabel = label || ui('快捷预设:', 'Presets:');

  const [isAdding, setIsAdding] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [nameInput, setNameInput] = useState('');

  const handleStartAdd = () => {
    setNameInput(currentModel.trim() || ui('自定义预设', 'Custom preset'));
    setIsAdding(true);
  };

  const sanitizeForApply = async (preset: PresetItem): Promise<PresetItem> => {
    const endpoint = preset.baseUrl.trim() || DEFAULT_AI_ENDPOINT;
    const expectedCredentialRef = await credentialRefForEndpoint(endpoint, credentialScope);
    return {
      id: preset.id,
      name: preset.name,
      model: preset.model,
      baseUrl: endpoint,
      credentialRef: expectedCredentialRef,
    };
  };

  const handleConfirmAdd = async () => {
    setIsSaving(true);
    const finalName = nameInput.trim() || currentModel.trim() || ui('自定义预设', 'Custom preset');
    try {
      const endpoint = currentBaseUrl.trim() || DEFAULT_AI_ENDPOINT;
      const credentialRef = await credentialRefForEndpoint(endpoint, credentialScope);
      await onAdd({
        name: finalName,
        model: currentModel.trim(),
        baseUrl: endpoint,
        credentialRef,
      });
      setIsAdding(false);
      setNameInput('');
    } finally {
      setIsSaving(false);
    }
  };

  const handleKeyDown = async (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      await handleConfirmAdd();
    } else if (e.key === 'Escape') {
      setIsAdding(false);
    }
  };

  return (
    <div className="flex flex-wrap items-center gap-2 py-0.5">
      <span className="text-slate-500 text-xs font-medium shrink-0">{resolvedLabel}</span>

      {/* Preset Badges */}
      {presets.map((p, idx) => (
        <div
          key={p.id || `${p.name}-${idx}`}
          className="group flex items-center gap-1 pl-2.5 pr-1.5 py-1 rounded bg-slate-100 dark:bg-slate-700/80 hover:bg-slate-200/90 dark:hover:bg-slate-700 border border-slate-200/80 dark:border-slate-600 transition"
        >
          <button
            type="button"
            onClick={async () => onApply(await sanitizeForApply(p))}
            className="text-xs text-slate-700 dark:text-slate-200 hover:text-blue-600 dark:hover:text-blue-400 font-mono font-medium truncate max-w-[160px] cursor-pointer"
            title={ui(
              `点击应用预设\n模型: ${p.model}\n端点: ${p.baseUrl}`,
              `Apply preset\nModel: ${p.model}\nEndpoint: ${p.baseUrl}`
            )}
          >
            {p.name}
          </button>
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              onDelete(idx);
            }}
            aria-label={ui(`删除预设 ${p.name}`, `Delete preset ${p.name}`)}
            className="p-0.5 rounded text-slate-400 hover:text-rose-500 hover:bg-slate-200 dark:hover:bg-slate-600 transition cursor-pointer"
            title={ui('删除此预设', 'Delete preset')}
          >
            <X className="w-3 h-3" />
          </button>
        </div>
      ))}

      {/* Empty State */}
      {presets.length === 0 && !isAdding && (
        <span className="text-slate-400 dark:text-slate-500 text-xs">
          {ui(`暂无预设 (上限 ${maxCount} 个)`, `No presets (max ${maxCount})`)}
        </span>
      )}

      {/* Inline Add Input */}
      {isAdding ? (
        <div className="flex items-center gap-1.5 bg-white dark:bg-slate-900 p-1 rounded border border-blue-500 shadow-xs animate-in fade-in-50 duration-100">
          <input
            type="text"
            value={nameInput}
            onChange={(e) => setNameInput(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder={ui('预设名称', 'Preset name')}
            autoFocus
            className="px-1.5 py-0.5 text-xs font-mono rounded bg-slate-50 dark:bg-slate-800 text-slate-800 dark:text-slate-200 border border-slate-200 dark:border-slate-700 focus:outline-none w-36"
          />
          <button
            type="button"
            onClick={handleConfirmAdd}
            disabled={isSaving}
            className="px-2 py-0.5 rounded bg-blue-600 hover:bg-blue-700 text-white text-xs font-medium transition cursor-pointer"
          >
            {ui('保存', 'Save')}
          </button>
          <button
            type="button"
            onClick={() => setIsAdding(false)}
            className="px-1.5 py-0.5 rounded text-slate-400 hover:text-slate-600 dark:hover:text-slate-200 text-xs transition cursor-pointer"
          >
            {t('export.cancel')}
          </button>
        </div>
      ) : (
        presets.length < maxCount && (
          <button
            type="button"
            onClick={handleStartAdd}
            className="flex items-center gap-1 px-2 py-1 rounded border border-dashed border-slate-300 dark:border-slate-600 text-slate-500 dark:text-slate-400 hover:border-blue-500 hover:text-blue-600 dark:hover:text-blue-400 text-xs transition cursor-pointer"
            title={ui('将当前模型、端点与 API Key 存为快捷预设', 'Save current endpoint and model as preset')}
          >
            <BookmarkPlus className="w-3 h-3" />
            <span>{ui('存为预设', 'Save preset')}</span>
          </button>
        )
      )}
    </div>
  );
};
