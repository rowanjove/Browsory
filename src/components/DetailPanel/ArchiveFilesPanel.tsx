import React, { useEffect, useRef, useState } from 'react';
import { Download, File, Image as ImageIcon, Loader2, ShieldCheck } from 'lucide-react';
import { save } from '@tauri-apps/plugin-dialog';
import { archiveFiles, ArchiveFileInfo } from '../../services/archiveFiles';
import { useAppStore } from '../../stores/useAppStore';

interface ArchiveFilesPanelProps {
  pageUuid: string;
}

const formatBytes = (bytes: number) => bytes < 1024 ? `${bytes} B` : bytes < 1024 * 1024 ? `${(bytes / 1024).toFixed(1)} KB` : `${(bytes / (1024 * 1024)).toFixed(2)} MB`;

export const ArchiveFilesPanel: React.FC<ArchiveFilesPanelProps> = ({ pageUuid }) => {
  const { showToast, language, t } = useAppStore();
  const ui = (zh: string, en: string) => (language === 'en-US' ? en : zh);
  const [files, setFiles] = useState<ArchiveFileInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [reloadToken, setReloadToken] = useState(0);
  const [busy, setBusy] = useState<string | null>(null);
  const [previewUrl, setPreviewUrl] = useState<string | null>(null);
  const pageEpochRef = useRef(0);
  const mountedRef = useRef(true);
  const previewUrlRef = useRef<string | null>(null);

  const releasePreviewUrl = () => {
    if (previewUrlRef.current) {
      URL.revokeObjectURL(previewUrlRef.current);
      previewUrlRef.current = null;
    }
  };

  const errorText = (error: unknown, fallback: string) => {
    if (typeof error === 'string' && error.trim()) return error;
    if (error && typeof error === 'object' && 'message' in error && typeof error.message === 'string' && error.message.trim()) return error.message;
    return fallback;
  };

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      pageEpochRef.current += 1;
      releasePreviewUrl();
    };
  }, []);

  useEffect(() => {
    const epoch = ++pageEpochRef.current;
    let active = true;
    setLoading(true);
    setLoadError(null);
    setFiles([]);
    setBusy(null);
    releasePreviewUrl();
    setPreviewUrl(null);
    archiveFiles.list(pageUuid)
      .then((value) => { if (active && mountedRef.current && pageEpochRef.current === epoch) setFiles(value); })
      .catch((error) => { if (active && mountedRef.current && pageEpochRef.current === epoch) setLoadError(errorText(error, ui('附件索引读取失败', 'Failed to read attachment index'))); })
      .finally(() => { if (active && mountedRef.current && pageEpochRef.current === epoch) setLoading(false); });
    return () => {
      active = false;
      if (pageEpochRef.current === epoch) pageEpochRef.current += 1;
      releasePreviewUrl();
    };
  }, [pageUuid, reloadToken]);

  const preview = async (file: ArchiveFileInfo) => {
    const epoch = pageEpochRef.current;
    setBusy(file.relative_name);
    try {
      const result = await archiveFiles.preview(pageUuid, file.relative_name);
      if (!mountedRef.current || pageEpochRef.current !== epoch) return;
      const objectUrl = URL.createObjectURL(new Blob([new Uint8Array(result.bytes)], { type: result.mime_type }));
      if (!mountedRef.current || pageEpochRef.current !== epoch) { URL.revokeObjectURL(objectUrl); return; }
      releasePreviewUrl();
      previewUrlRef.current = objectUrl;
      setPreviewUrl(objectUrl);
    } catch (error) {
      if (mountedRef.current && pageEpochRef.current === epoch) showToast(errorText(error, ui('图片预览失败', 'Failed to preview image')), 'error');
    } finally {
      if (mountedRef.current && pageEpochRef.current === epoch) setBusy(null);
    }
  };

  const exportFile = async (file: ArchiveFileInfo) => {
    const epoch = pageEpochRef.current;
    setBusy(file.relative_name);
    try {
      const target = await save({ defaultPath: file.relative_name.split('/').pop() || 'archive-file' });
      if (!target) return;
      if (!mountedRef.current || pageEpochRef.current !== epoch) return;
      await archiveFiles.export(pageUuid, file.relative_name, target);
      if (mountedRef.current && pageEpochRef.current === epoch) showToast(ui('附件已导出到你选择的位置', 'Attachment exported'), 'success');
    } catch (error) {
      if (mountedRef.current && pageEpochRef.current === epoch) showToast(errorText(error, ui('附件导出失败', 'Failed to export attachment')), 'error');
    } finally {
      if (mountedRef.current && pageEpochRef.current === epoch) setBusy(null);
    }
  };

  return <section className="space-y-3 pt-4 border-t border-slate-200 dark:border-slate-700">
    <div className="flex items-center justify-between gap-3">
      <div>
        <h4 className="text-sm font-bold text-slate-900 dark:text-slate-100">{ui('加密附件', 'Encrypted attachments')}</h4>
        <p className="text-xs text-slate-500 dark:text-slate-400">
          {ui(
            '旧网页截图和附件已从 SQLCipher 中读取；只有你明确选择导出位置时才写出明文文件。',
            'Archived screenshots and attachments stay inside SQLCipher until you explicitly choose an export destination.'
          )}
        </p>
      </div>
      <ShieldCheck className="w-4 h-4 text-emerald-500 shrink-0" />
    </div>
    {loading ? <div className="text-xs text-slate-400 flex items-center gap-2"><Loader2 className="w-3.5 h-3.5 animate-spin" />{ui('正在读取附件索引…', 'Loading attachment index...')}</div> : loadError ? <div className="space-y-2 text-xs text-rose-600 dark:text-rose-400"><p>{loadError}</p><button type="button" onClick={() => setReloadToken((value) => value + 1)} className="px-3 py-1.5 rounded-lg border border-rose-200 dark:border-rose-800 hover:bg-rose-50 dark:hover:bg-rose-950/30">{t('common.retry')}</button></div> : files.length === 0 ? <p className="text-xs text-slate-400">{ui('此页面没有已迁移的附件。', 'No migrated attachments for this page.')}</p> : <>
      <div className="space-y-2">{files.map((file) => <div key={file.relative_name} className="flex items-center gap-2 p-2 rounded-xl bg-slate-50 dark:bg-slate-800 border border-slate-200 dark:border-slate-700 text-xs">
        {file.kind === 'image' ? <ImageIcon className="w-4 h-4 text-blue-500 shrink-0" /> : <File className="w-4 h-4 text-slate-400 shrink-0" />}
        <span className="min-w-0 flex-1 truncate" title={file.relative_name}>{file.relative_name}</span><span className="text-slate-400 shrink-0">{formatBytes(file.size_bytes)}</span>
        {file.kind === 'image' && <button type="button" onClick={() => preview(file)} disabled={busy !== null} className="text-blue-600 hover:text-blue-700 disabled:opacity-50 cursor-pointer">{busy === file.relative_name ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : ui('预览', 'Preview')}</button>}
        <button type="button" onClick={() => exportFile(file)} disabled={busy !== null} title={ui('导出到你选择的位置', 'Export to chosen path')} aria-label={ui('导出附件', 'Export attachment')} className="text-slate-500 hover:text-blue-600 disabled:opacity-50 cursor-pointer"><Download className="w-3.5 h-3.5" /></button>
      </div>)}</div>
      {previewUrl && <div className="rounded-xl border border-slate-200 dark:border-slate-700 p-2 bg-white dark:bg-slate-900"><img src={previewUrl} alt={ui('归档截图预览', 'Archived screenshot preview')} className="max-h-72 max-w-full mx-auto object-contain" /></div>}
    </>}
  </section>;
};
