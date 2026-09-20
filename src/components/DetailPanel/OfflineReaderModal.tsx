import React, { useState, useEffect } from 'react';
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

export const OfflineReaderModal: React.FC<OfflineReaderModalProps> = ({
  isOpen,
  urlId,
  url,
  title,
  onClose,
}) => {
  const { showToast } = useAppStore();

  const [activeTab, setActiveTab] = useState<TabType>('reader');
  const [archiveDetail, setArchiveDetail] = useState<PageArchiveDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [copied, setCopied] = useState(false);

  const loadArchive = async () => {
    setLoading(true);
    try {
      const res = await tauriApi.getOfflineArchive(urlId);
      setArchiveDetail(res);
      if (res && !res.markdown_content && res.snapshot_html) {
        setActiveTab('snapshot');
      }
    } catch (err) {
      console.error('Failed to load offline archive:', err);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    if (isOpen && urlId) {
      loadArchive();
    }
  }, [isOpen, urlId]);

  if (!isOpen) return null;

  const handleManualCreateArchive = async () => {
    setSaving(true);
    try {
      // This action records local metadata only. It must not claim to have fetched
      // the remote page when no browser capture payload was supplied.
      const safeTitle = escapeHtml(title || url);
      const safeUrl = escapeHtml(url);
      const archivedAt = new Date().toLocaleString();
      const initialMarkdown = `# ${title || url}\n\n来源网址: ${url}\n记录时间: ${archivedAt}\n\n> 这是本地阅读索引记录。当前版本未抓取远程网页正文或资源。`;
      const initialHtml = `<!DOCTYPE html><html><head><meta charset="utf-8"><title>${safeTitle}</title><style>body{font-family:system-ui,-apple-system,sans-serif;max-width:720px;margin:40px auto;padding:0 20px;line-height:1.6;color:#1e293b}</style></head><body><h1>${safeTitle}</h1><p><strong>URL:</strong> ${safeUrl}</p><p><strong>记录时间:</strong> ${archivedAt}</p><hr><p>这是本地阅读索引记录，不包含远程网页正文或资源。</p></body></html>`;

      await tauriApi.saveOfflineArchive({
        url_id: urlId,
        archive_level: 'content',
        markdown: initialMarkdown,
        html: initialHtml,
        title,
      });

      showToast('已保存本地阅读索引（未抓取正文）', 'success');
      await loadArchive();
    } catch (err) {
      showToast(typeof err === 'string' ? err : '创建离线快照失败', 'error');
    } finally {
      setSaving(false);
    }
  };

  const handleDeleteArchive = async () => {
    if (!archiveDetail) return;
    try {
      await tauriApi.deleteOfflineArchive(archiveDetail.summary.page_uuid);
      showToast('已移除该页面的离线归档包', 'success');
      setArchiveDetail(null);
    } catch (err) {
      showToast(typeof err === 'string' ? err : '删除归档失败', 'error');
    }
  };

  const handleCopyMarkdown = () => {
    if (!archiveDetail?.markdown_content) return;
    navigator.clipboard.writeText(archiveDetail.markdown_content).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    });
  };

  const formatBytes = (bytes: number) => {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/70 backdrop-blur-sm animate-in fade-in duration-200 select-none">
      <div className="w-full max-w-4xl h-[85vh] bg-white dark:bg-slate-900 border border-slate-200 dark:border-slate-800 rounded-3xl shadow-2xl flex flex-col overflow-hidden">
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
                  <span className="px-2 py-0.5 rounded-full text-[10px] font-semibold bg-emerald-50 dark:bg-emerald-950/60 text-emerald-600 dark:text-emerald-400 border border-emerald-200 dark:border-emerald-800 shrink-0">
                    已归档 ({archiveDetail.summary.archive_level})
                  </span>
                )}
              </div>
              <p className="text-xs text-slate-400 truncate max-w-md font-mono mt-0.5">
                {url}
              </p>
            </div>
          </div>

          <div className="flex items-center gap-2">
            {/* Nav Tabs */}
            {archiveDetail && (
              <div className="flex items-center bg-slate-100 dark:bg-slate-800 p-1 rounded-xl text-xs">
                <button
                  type="button"
                  onClick={() => setActiveTab('reader')}
                  className={`px-3 py-1.5 rounded-lg font-medium transition ${
                    activeTab === 'reader'
                      ? 'bg-white dark:bg-slate-700 text-slate-900 dark:text-slate-100 shadow-xs'
                      : 'text-slate-500 hover:text-slate-800 dark:hover:text-slate-200'
                  }`}
                >
                  正文阅读
                </button>
                <button
                  type="button"
                  onClick={() => setActiveTab('snapshot')}
                  className={`px-3 py-1.5 rounded-lg font-medium transition ${
                    activeTab === 'snapshot'
                      ? 'bg-white dark:bg-slate-700 text-slate-900 dark:text-slate-100 shadow-xs'
                      : 'text-slate-500 hover:text-slate-800 dark:hover:text-slate-200'
                  }`}
                >
                  HTML 记录
                </button>
                <button
                  type="button"
                  onClick={() => setActiveTab('meta')}
                  className={`px-3 py-1.5 rounded-lg font-medium transition ${
                    activeTab === 'meta'
                      ? 'bg-white dark:bg-slate-700 text-slate-900 dark:text-slate-100 shadow-xs'
                      : 'text-slate-500 hover:text-slate-800 dark:hover:text-slate-200'
                  }`}
                >
                  归档包信息
                </button>
              </div>
            )}

            <button
              type="button"
              onClick={() => tauriApi.openExternalUrl(url)}
              className="p-2 rounded-xl hover:bg-slate-100 dark:hover:bg-slate-800 text-slate-500 hover:text-slate-800 dark:hover:text-slate-200 transition"
              title="在浏览器中打开原网页"
            >
              <ExternalLink className="w-4 h-4" />
            </button>

            <button
              type="button"
              onClick={onClose}
              className="p-2 rounded-xl hover:bg-slate-100 dark:hover:bg-slate-800 text-slate-400 hover:text-slate-600 dark:hover:text-slate-200 transition"
            >
              <X className="w-5 h-5" />
            </button>
          </div>
        </div>

        {/* Modal Main Content Area */}
        <div className="flex-1 overflow-hidden relative">
          {loading ? (
            <div className="h-full flex flex-col items-center justify-center gap-3 text-slate-400">
              <Loader2 className="w-6 h-6 animate-spin text-blue-500" />
              <span className="text-xs">正在从本地独立归档包加载页面...</span>
            </div>
          ) : !archiveDetail ? (
            /* Empty State: Prompt to Archive */
            <div className="h-full flex flex-col items-center justify-center text-center p-8 space-y-4 max-w-md mx-auto">
              <div className="w-14 h-14 rounded-2xl bg-slate-100 dark:bg-slate-800 border border-slate-200 dark:border-slate-700 flex items-center justify-center text-slate-400">
                <HardDrive className="w-7 h-7" />
              </div>
              <div className="space-y-1.5">
                <h3 className="text-base font-bold text-slate-800 dark:text-slate-200">
                  尚未保存本地阅读索引
                </h3>
                <p className="text-xs text-slate-500 dark:text-slate-400 leading-relaxed">
                  当前版本只保存本地索引和元数据，不会伪装成已抓取远程正文。真实网页归档需要浏览器捕获数据。
                </p>
              </div>

              <button
                type="button"
                onClick={handleManualCreateArchive}
                disabled={saving}
                className="px-5 py-2.5 rounded-xl bg-blue-600 hover:bg-blue-700 text-white text-xs font-semibold shadow-sm transition flex items-center gap-2 cursor-pointer disabled:opacity-50"
              >
                {saving ? (
                  <Loader2 className="w-4 h-4 animate-spin" />
                ) : (
                  <Sparkles className="w-4 h-4" />
                )}
                <span>{saving ? '正在保存本地索引...' : '保存本地阅读索引'}</span>
              </button>
            </div>
          ) : (
            /* Render active tab */
            <div className="h-full overflow-hidden flex flex-col">
              {/* Tab 1: Reader Mode (Markdown) */}
              {activeTab === 'reader' && (
                <div className="h-full flex flex-col">
                  {/* Toolbar */}
                  <div className="px-6 py-2 bg-slate-50 dark:bg-slate-800/60 border-b border-slate-100 dark:border-slate-800 flex items-center justify-between text-xs text-slate-500">
                    <div className="flex items-center gap-2">
                      <FileText className="w-3.5 h-3.5 text-blue-500" />
                    <span>本地索引阅读模式</span>
                    </div>
                    <button
                      type="button"
                      onClick={handleCopyMarkdown}
                      className="flex items-center gap-1 text-slate-600 dark:text-slate-300 hover:text-blue-600"
                    >
                      {copied ? <Check className="w-3.5 h-3.5 text-emerald-500" /> : <Copy className="w-3.5 h-3.5" />}
                      <span>{copied ? '已复制 Markdown' : '复制 Markdown'}</span>
                    </button>
                  </div>

                  {/* Body Content */}
                  <div className="flex-1 overflow-y-auto p-8 select-text">
                    <article className="max-w-2xl mx-auto prose dark:prose-invert text-slate-800 dark:text-slate-200">
                      <pre className="whitespace-pre-wrap font-sans text-sm leading-relaxed">
                        {archiveDetail.markdown_content || '未找到正文 Markdown 内容。'}
                      </pre>
                    </article>
                  </div>
                </div>
              )}

              {/* Tab 2: HTML Snapshot (Sandboxed Iframe) */}
              {activeTab === 'snapshot' && (
                <div className="h-full flex flex-col">
                  <div className="px-6 py-2 bg-amber-50 dark:bg-amber-950/40 border-b border-amber-200 dark:border-amber-800 flex items-center justify-between text-[11px] text-amber-800 dark:text-amber-300">
                    <div className="flex items-center gap-1.5">
                      <ShieldAlert className="w-3.5 h-3.5" />
                      <span>沙箱渲染：已禁用脚本执行；归档内容仍按不可信网页处理</span>
                    </div>
                  </div>
                  <iframe
                    title="HTML Snapshot"
                    srcDoc={archiveDetail.snapshot_html || '<html><body>未找到快照 HTML</body></html>'}
                    sandbox="allow-same-origin"
                    className="w-full flex-1 border-0 bg-white"
                  />
                </div>
              )}

              {/* Tab 3: Package Metadata */}
              {activeTab === 'meta' && (
                <div className="h-full overflow-y-auto p-8 space-y-4 max-w-lg mx-auto">
                  <h3 className="text-sm font-bold text-slate-900 dark:text-slate-100">
                    页面归档信息
                  </h3>

                  <div className="p-4 rounded-2xl bg-slate-50 dark:bg-slate-800 border border-slate-200 dark:border-slate-700 space-y-2.5 font-mono text-xs">
                    <div className="flex justify-between">
                      <span className="text-slate-400">Page UUID:</span>
                      <span className="text-slate-800 dark:text-slate-200 select-all">
                        {archiveDetail.summary.page_uuid}
                      </span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-slate-400">相对存储路径:</span>
                      <span className="text-slate-800 dark:text-slate-200">
                        {archiveDetail.summary.relative_path}
                      </span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-slate-400">归档层级:</span>
                      <span className="text-slate-800 dark:text-slate-200 uppercase font-bold">
                        {archiveDetail.summary.archive_level}
                      </span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-slate-400">磁盘物理占用:</span>
                      <span className="text-slate-800 dark:text-slate-200">
                        {formatBytes(archiveDetail.summary.size_bytes)}
                      </span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-slate-400">归档时间:</span>
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
                      <span>删除此页面的离线归档包</span>
                    </button>
                  </div>
                </div>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
};
