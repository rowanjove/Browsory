import React from 'react';
import {
  X,
  Sparkles,
  ExternalLink,
  ShieldCheck,
  Globe,
  Search,
  Lock,
  FileSpreadsheet,
  Brain,
  Github,
} from 'lucide-react';
import { AppLogo } from '../Common/AppLogo';
import { useAppStore } from '../../stores/useAppStore';
import { tauriApi } from '../../services/tauri';
import { useDialogFocus } from '../../hooks/useDialogFocus';

export const ChangelogModal: React.FC = () => {
  const { isChangelogOpen, closeChangelog, t, appVersion } = useAppStore();
  const dialogRef = useDialogFocus<HTMLDivElement>({ isOpen: isChangelogOpen, onClose: closeChangelog });

  if (!isChangelogOpen) return null;

  const features = [
    {
      icon: <Globe className="w-4 h-4 text-blue-500" />,
      title: t('changelog.feature1Title'),
      description: t('changelog.feature1Description'),
    },
    {
      icon: <Search className="w-4 h-4 text-indigo-500" />,
      title: t('changelog.feature2Title'),
      description: t('changelog.feature2Description'),
    },
    {
      icon: <Sparkles className="w-4 h-4 text-emerald-500" />,
      title: t('changelog.feature3Title'),
      description: t('changelog.feature3Description'),
    },
    {
      icon: <Lock className="w-4 h-4 text-amber-500" />,
      title: t('changelog.feature4Title'),
      description: t('changelog.feature4Description'),
    },
    {
      icon: <FileSpreadsheet className="w-4 h-4 text-cyan-500" />,
      title: t('changelog.feature5Title'),
      description: t('changelog.feature5Description'),
    },
    {
      icon: <Brain className="w-4 h-4 text-purple-500" />,
      title: t('changelog.feature6Title'),
      description: t('changelog.feature6Description'),
    },
  ];

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/60 backdrop-blur-sm p-4 text-xs select-none animate-in fade-in duration-150">
      <div
        ref={dialogRef}
        className="w-full max-w-2xl max-h-[85vh] bg-white dark:bg-slate-900 border border-slate-200 dark:border-slate-800 rounded-2xl shadow-2xl flex flex-col overflow-hidden animate-in zoom-in-95 duration-150"
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-labelledby="changelog-title"
        tabIndex={-1}
      >
        {/* Header */}
        <div className="px-6 py-4 border-b border-slate-100 dark:border-slate-800 bg-slate-50/50 dark:bg-slate-800/50 flex items-center justify-between shrink-0">
          <div className="flex items-center gap-3">
            <AppLogo size={32} className="shrink-0 drop-shadow-sm" />
            <div>
              <div className="flex items-center gap-2">
                <h2 id="changelog-title" className="text-sm font-bold text-slate-900 dark:text-slate-100">
                  {t('changelog.title')}
                </h2>
                <span className="px-2 py-0.5 rounded-full text-[10px] font-semibold font-mono bg-blue-100 dark:bg-blue-950/80 text-blue-700 dark:text-blue-300 border border-blue-200 dark:border-blue-800">
                  v{appVersion}
                </span>
              </div>
              <p className="text-[11px] text-slate-500 dark:text-slate-400 mt-0.5">
                {t('changelog.subtitle')}
              </p>
            </div>
          </div>

          <button
            type="button"
            onClick={closeChangelog}
            aria-label={t('changelog.close')}
            className="p-1.5 rounded-lg text-slate-400 hover:text-slate-600 dark:hover:text-slate-200 hover:bg-slate-100 dark:hover:bg-slate-800 transition cursor-pointer"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Content List */}
        <div className="flex-1 overflow-y-auto p-6 space-y-4">
          <div className="grid grid-cols-1 gap-3">
            {features.map((feat, idx) => (
              <div
                key={idx}
                className="p-3.5 rounded-xl bg-slate-50/70 dark:bg-slate-800/50 border border-slate-100 dark:border-slate-800/80 flex items-start gap-3.5"
              >
                <div className="p-2 rounded-lg bg-white dark:bg-slate-800 border border-slate-200/60 dark:border-slate-700 shadow-2xs shrink-0 mt-0.5">
                  {feat.icon}
                </div>
                <div className="flex flex-col gap-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="text-xs font-semibold text-slate-800 dark:text-slate-100">
                      {idx + 1}. {feat.title}
                    </span>
                  </div>
                  <p className="text-[11px] text-slate-500 dark:text-slate-400 leading-relaxed">
                    {feat.description}
                  </p>
                </div>
              </div>
            ))}
          </div>
        </div>

        {/* Footer */}
        <div className="px-6 py-3.5 border-t border-slate-100 dark:border-slate-800 bg-slate-50/50 dark:bg-slate-800/50 flex items-center justify-between shrink-0">
          <div className="flex items-center gap-3 text-[11px] text-slate-400">
            <span className="flex items-center gap-1">
              <ShieldCheck className="w-3.5 h-3.5 text-emerald-500" />
              <span>{t('changelog.license')}</span>
            </span>
          </div>

          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => tauriApi.openExternalUrl('https://github.com/rowanjove/Browsory')}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-slate-200 dark:border-slate-700 hover:bg-white dark:hover:bg-slate-800 text-slate-700 dark:text-slate-300 transition cursor-pointer text-xs font-medium"
            >
              <Github className="w-3.5 h-3.5" />
              <span>{t('changelog.github')}</span>
              <ExternalLink className="w-3 h-3 text-slate-400" />
            </button>

            <button
              type="button"
              onClick={closeChangelog}
              className="px-4 py-1.5 rounded-lg bg-blue-600 hover:bg-blue-700 text-white font-medium transition cursor-pointer text-xs shadow-xs"
            >
              {t('changelog.done')}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};
