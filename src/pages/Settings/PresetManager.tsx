import React, { useState } from 'react';
import { X, BookmarkPlus } from 'lucide-react';

export interface PresetItem {
  id?: string;
  name: string;
  model: string;
  baseUrl: string;
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
  currentApiKey?: string;
  maxCount?: number;
}

export const PresetManager: React.FC<PresetManagerProps> = ({
  label = '快捷预设:',
  presets,
  onApply,
  onAdd,
  onDelete,
  currentModel,
  currentBaseUrl,
  currentApiKey,
  maxCount = 3,
}) => {
  const [isAdding, setIsAdding] = useState(false);
  const [nameInput, setNameInput] = useState('');

  const handleStartAdd = () => {
    setNameInput(currentModel.trim() || '自定义预设');
    setIsAdding(true);
  };

  const handleConfirmAdd = () => {
    const finalName = nameInput.trim() || currentModel.trim() || '自定义预设';
    onAdd({
      name: finalName,
      model: currentModel.trim(),
      baseUrl: currentBaseUrl.trim(),
      apiKey: currentApiKey ? currentApiKey.trim() : undefined,
    });
    setIsAdding(false);
    setNameInput('');
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      handleConfirmAdd();
    } else if (e.key === 'Escape') {
      setIsAdding(false);
    }
  };

  return (
    <div className="flex flex-wrap items-center gap-2 py-0.5">
      <span className="text-slate-500 text-[11px] font-medium shrink-0">{label}</span>

      {/* Preset Badges */}
      {presets.map((p, idx) => (
        <div
          key={p.id || `${p.name}-${idx}`}
          className="group flex items-center gap-1 pl-2.5 pr-1.5 py-1 rounded bg-slate-100 dark:bg-slate-700/80 hover:bg-slate-200/90 dark:hover:bg-slate-700 border border-slate-200/80 dark:border-slate-600 transition"
        >
          <button
            type="button"
            onClick={() => onApply(p)}
            className="text-[11px] text-slate-700 dark:text-slate-200 hover:text-blue-600 dark:hover:text-blue-400 font-mono font-medium truncate max-w-[140px] cursor-pointer"
            title={`点击应用预设\n模型: ${p.model}\n端点: ${p.baseUrl}`}
          >
            {p.name}
          </button>
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              onDelete(idx);
            }}
            className="p-0.5 rounded text-slate-400 hover:text-rose-500 hover:bg-slate-200 dark:hover:bg-slate-600 transition cursor-pointer"
            title="删除此预设"
          >
            <X className="w-3 h-3" />
          </button>
        </div>
      ))}

      {/* Empty State */}
      {presets.length === 0 && !isAdding && (
        <span className="text-slate-400 dark:text-slate-500 text-[11px]">
          暂无预设 (上限 {maxCount} 个)
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
            placeholder="预设名称"
            autoFocus
            className="px-1.5 py-0.5 text-[11px] font-mono rounded bg-slate-50 dark:bg-slate-800 text-slate-800 dark:text-slate-200 border border-slate-200 dark:border-slate-700 focus:outline-none w-32"
          />
          <button
            type="button"
            onClick={handleConfirmAdd}
            className="px-2 py-0.5 rounded bg-blue-600 hover:bg-blue-700 text-white text-[11px] font-medium transition cursor-pointer"
          >
            保存
          </button>
          <button
            type="button"
            onClick={() => setIsAdding(false)}
            className="px-1.5 py-0.5 rounded text-slate-400 hover:text-slate-600 dark:hover:text-slate-200 text-[11px] transition cursor-pointer"
          >
            取消
          </button>
        </div>
      ) : (
        presets.length < maxCount && (
          <button
            type="button"
            onClick={handleStartAdd}
            className="flex items-center gap-1 px-2 py-1 rounded border border-dashed border-slate-300 dark:border-slate-700 hover:border-blue-400 dark:hover:border-blue-500 hover:bg-blue-50/50 dark:hover:bg-blue-950/30 text-slate-500 hover:text-blue-600 dark:hover:text-blue-400 text-[11px] transition cursor-pointer"
            title="将当前已输入的配置保存为新预设"
          >
            <BookmarkPlus className="w-3 h-3" />
            <span>保存当前为预设 ({presets.length}/{maxCount})</span>
          </button>
        )
      )}
    </div>
  );
};
