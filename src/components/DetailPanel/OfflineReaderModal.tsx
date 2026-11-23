import React, { useState, useEffect, useRef } from 'react';
import {
  BookOpen,
  X,
  FileText,
  Copy,
  Check,
  Trash2,
  ExternalLink,
  ShieldAlert,
  Loader2,
  HardDrive,
  Sparkles,
} from 'lucide-react';
import { tauriApi } from '../../services/tauri';
import { useAppStore } from '../../stores/useAppStore';
import { PageArchiveDetail } from '../../types';
import { ArchiveFilesPanel } from './ArchiveFilesPanel';
import { createRequestSequence } from '../../services/requestSequence';
import { useDialogFocus } from '../../hooks/useDialogFocus';

interface OfflineReaderModalProps {
  isOpen: boolean;
  urlId: number;
  url: string;
  title: string;
  onClose: () => void;
}

type TabType = 'reader' | 'snapshot' | 'meta';

const escapeHtml = (value: string) =>
  value.replace(/[&<>"']/g, (char) => ({
    '&': '&amp;',
    '<': '&lt;',
    '>': '&gt;',
    '"': '&quot;',
    "'": '&#39;',
  })[char] as string);

const PASSIVE_TAGS = new Set([
  'a', 'address', 'article', 'aside', 'b', 'blockquote', 'br', 'caption', 'code', 'col',
  'colgroup', 'dd', 'details', 'div', 'dl', 'dt', 'em', 'figcaption', 'figure', 'footer',
  'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'header', 'hr', 'i', 'img', 'li', 'main', 'mark',
  'ol', 'p', 'pre', 'q', 'section', 'small', 'source', 'span', 'strong', 'sub', 'summary',
  'sup', 'table', 'tbody', 'td', 'tfoot', 'th', 'thead', 'time', 'tr', 'u', 'ul', 'video',
  'audio',
]);
const DROP_TAGS = new Set([
  'base', 'button', 'canvas', 'embed', 'form', 'frame', 'frameset', 'iframe', 'input',
  'link', 'math', 'meta', 'noscript', 'object', 'option', 'portal', 'script', 'select',
  'style', 'svg', 'template', 'textarea', 'track',
]);
const SAFE_ATTRIBUTES = new Set([
  'alt', 'aria-label', 'class', 'colspan', 'controls', 'data-*', 'height', 'id', 'open',
  'preload', 'rowspan', 'scope', 'title', 'width',
]);
const OFFLINE_SNAPSHOT_CSP =
  `<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src data: blob:; media-src data: blob:; style-src 'unsafe-inline'; script-src 'none'; connect-src 'none'; frame-src 'none'; child-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'">`;

const isSafeMediaUrl = (value: string) => /^(?:blob:|data:(?:image|video|audio)\/)/i.test(value.trim());

/**
 * Parse in an inert template, then rebuild a strict passive tree. No parsed
 * resource-bearing element is ever inserted into the live document, and no
 * source/href/event/style attribute is copied except data/blob media sources.
 */
const clonePassiveNode = (node: Node): Node | null => {
  if (node.nodeType === Node.TEXT_NODE) return document.createTextNode(node.textContent || '');
  if (node.nodeType !== Node.ELEMENT_NODE) return null;

  const source = node as Element;
  const tag = source.tagName.toLowerCase();
  if (DROP_TAGS.has(tag)) return null;
  if (!PASSIVE_TAGS.has(tag)) {
    const fragment = document.createDocumentFragment();
    source.childNodes.forEach((child) => {
      const cloned = clonePassiveNode(child);
      if (cloned) fragment.appendChild(cloned);
    });
    return fragment;
  }

  const target = document.createElement(tag);
  for (const attribute of Array.from(source.attributes)) {
    const name = attribute.name.toLowerCase();
    if (name.startsWith('on') || name === 'style' || name === 'href' || name === 'srcset') continue;
    if ((name === 'data-src' || name === 'data-original') && isSafeMediaUrl(attribute.value)) {
      target.setAttribute('src', attribute.value);
      continue;
    }
    if (name === 'src' || name === 'poster') {
      if (isSafeMediaUrl(attribute.value)) target.setAttribute(name, attribute.value);
      continue;
    }
    if (SAFE_ATTRIBUTES.has(name) || (name.startsWith('data-') && name !== 'data-src')) {
      target.setAttribute(name, attribute.value);
    }
  }
  source.childNodes.forEach((child) => {
    const cloned = clonePassiveNode(child);
    if (cloned) target.appendChild(cloned);
  });
  return target;
};

export const buildOfflineSnapshotDocument = (snapshotHtml?: string) => {
  const template = document.createElement('template');
  template.innerHTML = snapshotHtml || '<p>未找到快照 HTML</p>';
  const sourceRoot = template.content.querySelector('body') || template.content;
  const body = document.createElement('body');
  sourceRoot.childNodes.forEach((child) => {
    const cloned = clonePassiveNode(child);
    if (cloned) body.appendChild(cloned);
  });
  return `<!doctype html><html><head><meta charset="utf-8">${OFFLINE_SNAPSHOT_CSP}<style>body{font-family:system-ui,-apple-system,sans-serif;max-width:900px;margin:24px auto;padding:0 20px;line-height:1.6;color:#1e293b}img,video,audio{max-width:100%;}</style></head>${body.outerHTML}</html>`;
};

export const OfflineReaderModal: React.FC<OfflineReaderModalProps> = ({
  isOpen,
  urlId,
  url,
  title,
  onClose,
}) => {
  const { showToast, language, t } = useAppStore();
  const ui = (zh: string, en: string) => (language === 'en-US' ? en : zh);
  const dialogRef = useDialogFocus<HTMLDivElement>({ isOpen, onClose });

  const [activeTab, setActiveTab] = useState<TabType>('reader');
  const [archiveDetail, setArchiveDetail] = useState<PageArchiveDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [copied, setCopied] = useState(false);
  const requestSequence = useRef(createRequestSequence());

  const loadArchive = async (requestedUrlId = urlId) => {
    const sequence = requestSequence.current.next();
    setLoading(true);
    setLoadError(null);
    setArchiveDetail(null);
    try {
      const res = await tauriApi.getOfflineArchive(requestedUrlId);
      if (!requestSequence.current.isCurrent(sequence) || requestedUrlId !== urlId) return;
      if (res && res.summary.url_id !== requestedUrlId) {
        throw new Error(ui('归档响应与当前页面不匹配', 'Archive response does not match this page'));
      }
      setArchiveDetail(res);
      if (res && !res.markdown_content && res.snapshot_html) {
        setActiveTab('snapshot');
      }
    } catch (err) {
      if (requestSequence.current.isCurrent(sequence)) {
        setArchiveDetail(null);
        setLoadError(typeof err === 'string' ? err : ui('读取离线归档失败，请重试', 'Failed to load archive; please retry'));
      }
    } finally {
      if (requestSequence.current.isCurrent(sequence)) setLoading(false);
    }
  };

  useEffect(() => {
    if (isOpen && urlId > 0) {
      loadArchive();
    }
    return () => {
      requestSequence.current.next();
    };
  }, [isOpen, urlId]);

  if (!isOpen) return null;

  const handleManualCreateArchive = async () => {
    setSaving(true);
    try {
      const safeTitle = escapeHtml(title || url);
      const safeUrl = escapeHtml(url);
      const archivedAt = new Date().toLocaleString();
      const initialMarkdown = `# ${title || url}\n\n${ui('来源网址', 'URL')}: ${url}\n${ui('记录时间', 'Saved')}: ${archivedAt}\n\n> ${ui('这是本地阅读索引记录。当前版本未抓取远程网页正文或资源。', 'Local reading index only. Remote webpage content is not fetched.')}`;
      const initialHtml = `<!DOCTYPE html><html><head><meta charset="utf-8"><title>${safeTitle}</title><style>body{font-family:system-ui,-apple-system,sans-serif;max-width:720px;margin:40px auto;padding:0 20px;line-height:1.6;color:#1e293b}</style></head><body><h1>${safeTitle}</h1><p><strong>URL:</strong> ${safeUrl}</p><p><strong>${ui('记录时间', 'Saved')}:</strong> ${archivedAt}</p><hr><p>${ui('这是本地阅读索引记录，不包含远程网页正文或资源。', 'Local reading index record; remote body and assets were not fetched.')}</p></body></html>`;

      await tauriApi.saveOfflineArchive({
        url_id: urlId,
        archive_level: 'content',
        markdown: initialMarkdown,
        html: initialHtml,
        title,
      });

      showToast(ui('已保存本地阅读索引（未抓取正文）', 'Saved local reading index (metadata only)'), 'success');
      await loadArchive();
    } catch (err) {
      showToast(typeof err === 'string' ? err : ui('创建离线快照失败', 'Failed to create local reading index'), 'error');
    } finally {
      setSaving(false);
    }
  };

  const handleDeleteArchive = async () => {
    if (!archiveDetail) return;
    try {
      await tauriApi.deleteOfflineArchive(archiveDetail.summary.page_uuid);
      showToast(ui('已移出阅读列表；旧版源文件及其加密迁移副本仍保留。', 'Removed from reading list; source archive artifacts remain untouched.'), 'success');
      setArchiveDetail(null);
    } catch (err) {
      showToast(typeof err === 'string' ? err : ui('删除归档失败', 'Failed to remove reading entry'), 'error');
    }
  };

  const handleCopyMarkdown = async () => {
    if (!archiveDetail?.markdown_content) return;
    try {
      await navigator.clipboard.writeText(archiveDetail.markdown_content);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
      showToast(ui('已复制 Markdown 到剪贴板', 'Markdown copied to clipboard'), 'success');
    } catch {
      showToast(ui('复制到剪贴板失败', 'Failed to copy to clipboard'), 'error');
    }
  };

  const formatBytes = (bytes: number) => {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/70 backdrop-blur-sm animate-in fade-in duration-200 select-none">
      <div ref={dialogRef} role="dialog" aria-modal="true" aria-label={ui('离线阅读器', 'Offline reader')} tabIndex={-1} className="w-full max-w-4xl h-[85vh] bg-white dark:bg-slate-900 border border-slate-200 dark:border-slate-800 rounded-3xl shadow-2xl flex flex-col overflow-hidden">
        {/* Modal Top Header */}
        <div className="px-6 py-4 border-b border-slate-100 dark:border-slate-800 flex items-center justify-between gap-4">
          <div className="flex items-center gap-3 min-w-0">
            <div className="w-9 h-9 rounded-xl bg-blue-50 dark:bg-blue-950/60 border border-blue-200 dark:border-blue-800 flex items-center justify-center text-blue-600 dark:text-blue-400 shrink-0">
              <BookOpen className="w-4 h-4" />
            </div>
            <div className="min-w-0">
              <div className="flex items-center gap-2">
                <span className="font-bold text-sm text-slate-900 dark:text-slate-100 truncate max-w-md">
                  {title || url}
                </span>
                {archiveDetail && (
                  <span className="px-2 py-0.5 rounded-full text-[11px] font-semibold bg-emerald-50 dark:bg-emerald-950/60 text-emerald-600 dark:text-emerald-400 border border-emerald-200 dark:border-emerald-800 shrink-0">
                    {ui(`已归档 (${archiveDetail.summary.archive_level})`, `Archived (${archiveDetail.summary.archive_level})`)}
                  </span>
                )}
              </div>
              <p className="text-xs text-slate-400 truncate max-w-md font-mono mt-0.5">
                {url}
              </p>
            </div>
          </div>

          <div className="flex items-center gap-2">
            {archiveDetail && (
              <div className="flex items-center bg-slate-100 dark:bg-slate-800 p-1 rounded-xl text-xs">
                <button
                  type="button"
                  onClick={() => setActiveTab('reader')}
                  className={`px-3 py-1.5 rounded-lg font-medium transition cursor-pointer ${
                    activeTab === 'reader'
                      ? 'bg-white dark:bg-slate-700 text-slate-900 dark:text-slate-100 shadow-xs'
                      : 'text-slate-500 hover:text-slate-800 dark:hover:text-slate-200'
                  }`}
                >
                  {ui('正文阅读', 'Reader')}
                </button>
                <button
                  type="button"
                  onClick={() => setActiveTab('snapshot')}
                  className={`px-3 py-1.5 rounded-lg font-medium transition cursor-pointer ${
                    activeTab === 'snapshot'
                      ? 'bg-white dark:bg-slate-700 text-slate-900 dark:text-slate-100 shadow-xs'
                      : 'text-slate-500 hover:text-slate-800 dark:hover:text-slate-200'
                  }`}
                >
                  {ui('HTML 记录', 'HTML view')}
                </button>
                <button
                  type="button"
                  onClick={() => setActiveTab('meta')}
                  className={`px-3 py-1.5 rounded-lg font-medium transition cursor-pointer ${
                    activeTab === 'meta'
                      ? 'bg-white dark:bg-slate-700 text-slate-900 dark:text-slate-100 shadow-xs'
                      : 'text-slate-500 hover:text-slate-800 dark:hover:text-slate-200'
                  }`}
                >
                  {ui('归档包信息', 'Metadata')}
                </button>
              </div>
            )}

            <button
              type="button"
              onClick={() => tauriApi.openExternalUrl(url)}
              aria-label={t('detail.openInBrowser')}
              className="p-2 rounded-xl hover:bg-slate-100 dark:hover:bg-slate-800 text-slate-500 hover:text-slate-800 dark:hover:text-slate-200 transition cursor-pointer"
              title={t('detail.openInBrowser')}
            >
              <ExternalLink className="w-4 h-4" />
            </button>

            <button
              type="button"
              onClick={onClose}
              aria-label={ui('关闭离线阅读器', 'Close offline reader')}
              className="p-2 rounded-xl hover:bg-slate-100 dark:hover:bg-slate-800 text-slate-400 hover:text-slate-600 dark:hover:text-slate-200 transition cursor-pointer"
            >
              <X className="w-5 h-5" />
            </button>
          </div>
        </div>

        <div className="flex-1 overflow-hidden relative">
          {loading ? (
            <div className="h-full flex flex-col items-center justify-center gap-3 text-slate-400">
              <Loader2 className="w-6 h-6 animate-spin text-blue-500" />
              <span className="text-xs">{ui('正在从本地独立归档包加载页面...', 'Loading local archive entry...')}</span>
            </div>
          ) : !archiveDetail ? (
            loadError ? (
              <div className="h-full flex flex-col items-center justify-center text-center p-8 space-y-4 max-w-md mx-auto">
                <div className="w-14 h-14 rounded-2xl bg-rose-50 dark:bg-rose-950/40 border border-rose-200 dark:border-rose-800 flex items-center justify-center text-rose-500">
                  <ShieldAlert className="w-7 h-7" />
                </div>
                <div className="space-y-1.5">
                  <h3 className="text-base font-bold text-slate-800 dark:text-slate-200">{ui('离线归档读取失败', 'Failed to load local archive')}</h3>
                  <p className="text-xs text-slate-500 dark:text-slate-400 leading-relaxed">{loadError}</p>
                </div>
                <button
                  type="button"
                  onClick={() => loadArchive(urlId)}
                  className="px-5 py-2.5 rounded-xl bg-blue-600 hover:bg-blue-700 text-white text-xs font-semibold shadow-sm transition cursor-pointer"
                >
                  {t('common.retry')}
                </button>
              </div>
            ) : (
            <div className="h-full flex flex-col items-center justify-center text-center p-8 space-y-4 max-w-md mx-auto">
              <div className="w-14 h-14 rounded-2xl bg-slate-100 dark:bg-slate-800 border border-slate-200 dark:border-slate-700 flex items-center justify-center text-slate-400">
                <HardDrive className="w-7 h-7" />
              </div>
              <div className="space-y-1.5">
                <h3 className="text-base font-bold text-slate-800 dark:text-slate-200">
                  {ui('尚未保存本地阅读索引', 'No local reading index yet')}
                </h3>
                <p className="text-xs text-slate-500 dark:text-slate-400 leading-relaxed">
                  {ui(
                    '当前版本只保存本地索引和元数据，不会伪装成已抓取远程正文。真实网页归档需要浏览器捕获数据。',
                    'Saves local metadata index only—remote webpage HTML is not fetched automatically.'
                  )}
                </p>
              </div>

              <button
                type="button"
                onClick={handleManualCreateArchive}
                disabled={saving}
                className="px-5 py-2.5 rounded-xl bg-blue-600 hover:bg-blue-700 text-white text-xs font-semibold shadow-sm transition flex items-center gap-2 cursor-pointer disabled:opacity-50"
              >
                {saving ? <Loader2 className="w-4 h-4 animate-spin" /> : <Sparkles className="w-4 h-4" />}
                <span>{saving ? ui('正在保存本地索引...', 'Saving local index...') : ui('保存本地阅读索引', 'Save local reading index')}</span>
              </button>
            </div>
            )
          ) : (
            <div className="h-full overflow-hidden flex flex-col">
              {activeTab === 'reader' && (
                <div className="h-full flex flex-col">
                  <div className="px-6 py-2 bg-slate-50 dark:bg-slate-800/60 border-b border-slate-100 dark:border-slate-800 flex items-center justify-between text-xs text-slate-500">
                    <div className="flex items-center gap-2">
                      <FileText className="w-3.5 h-3.5 text-blue-500" />
                      <span>{ui('本地索引阅读模式', 'Local reading index')}</span>
                    </div>
                    <button
                      type="button"
                      onClick={handleCopyMarkdown}
                      className="flex items-center gap-1 text-slate-600 dark:text-slate-300 hover:text-blue-600 cursor-pointer"
                    >
                      {copied ? <Check className="w-3.5 h-3.5 text-emerald-500" /> : <Copy className="w-3.5 h-3.5" />}
                      <span>{copied ? ui('已复制 Markdown', 'Copied Markdown') : ui('复制 Markdown', 'Copy Markdown')}</span>
                    </button>
                  </div>

                  <div className="flex-1 overflow-y-auto modal-scroll p-8 select-text">
                    <article className="max-w-2xl mx-auto prose dark:prose-invert text-slate-800 dark:text-slate-200">
                      <pre className="whitespace-pre-wrap font-sans text-sm leading-relaxed">
                        {archiveDetail.markdown_content || ui('未找到正文 Markdown 内容。', 'No Markdown content found.')}
                      </pre>
                    </article>
                  </div>
                </div>
              )}

              {activeTab === 'snapshot' && (
                <div className="h-full flex flex-col">
                  <div className="px-6 py-2 bg-amber-50 dark:bg-amber-950/40 border-b border-amber-200 dark:border-amber-800 flex items-center justify-between text-xs text-amber-800 dark:text-amber-300">
                    <div className="flex items-center gap-1.5">
                      <ShieldAlert className="w-3.5 h-3.5" />
                      <span>
                        {ui(
                          '离线安全渲染：仅显示结构化被动内容，已移除脚本、资源、表单、导航和刷新',
                          'Safe passive render: scripts, active forms, external assets, and navigation stripped'
                        )}
                      </span>
                    </div>
                  </div>
                  <iframe
                    title="HTML Snapshot"
                    srcDoc={buildOfflineSnapshotDocument(archiveDetail.snapshot_html || undefined)}
                    sandbox=""
                    referrerPolicy="no-referrer"
                    className="w-full flex-1 border-0 bg-white"
                  />
                </div>
              )}

              {activeTab === 'meta' && (
                <div className="h-full overflow-y-auto modal-scroll p-8 space-y-4 max-w-lg mx-auto">
                  <h3 className="text-sm font-bold text-slate-900 dark:text-slate-100">
                    {ui('页面归档信息', 'Archive entry metadata')}
                  </h3>

                  <div className="p-4 rounded-2xl bg-slate-50 dark:bg-slate-800 border border-slate-200 dark:border-slate-700 space-y-2.5 font-mono text-xs">
                    <div className="flex justify-between">
                      <span className="text-slate-400">Page UUID:</span>
                      <span className="text-slate-800 dark:text-slate-200 select-all">
                        {archiveDetail.summary.page_uuid}
                      </span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-slate-400">{ui('相对存储路径:', 'Relative path:')}</span>
                      <span className="text-slate-800 dark:text-slate-200">
                        {archiveDetail.summary.relative_path}
                      </span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-slate-400">{ui('归档层级:', 'Archive level:')}</span>
                      <span className="text-slate-800 dark:text-slate-200 uppercase font-bold">
                        {archiveDetail.summary.archive_level}
                      </span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-slate-400">{ui('磁盘物理占用:', 'Size:')}</span>
                      <span className="text-slate-800 dark:text-slate-200">
                        {formatBytes(archiveDetail.summary.size_bytes)}
                      </span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-slate-400">{ui('归档时间:', 'Archived at:')}</span>
                      <span className="text-slate-800 dark:text-slate-200">
                        {new Date(archiveDetail.summary.archived_at).toLocaleString()}
                      </span>
                    </div>
                  </div>

                  <div className="pt-4 flex justify-end">
                    <button
                      type="button"
                      onClick={handleDeleteArchive}
                      className="px-4 py-2 rounded-xl border border-rose-200 dark:border-rose-900 text-rose-600 dark:text-rose-400 hover:bg-rose-50 dark:hover:bg-rose-950/40 text-xs font-semibold transition flex items-center gap-1.5 cursor-pointer"
                    >
                      <Trash2 className="w-3.5 h-3.5" />
                      <span>{ui('删除此页面的离线归档包', 'Remove from reading list')}</span>
                    </button>
                  </div>
                  <ArchiveFilesPanel pageUuid={archiveDetail.summary.page_uuid} />
                </div>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
};
