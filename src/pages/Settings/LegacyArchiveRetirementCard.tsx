import React, { useEffect, useRef, useState } from 'react';
import { archiveRetirement, LegacySourcePage } from '../../services/archiveRetirement';
import { useAppStore } from '../../stores/useAppStore';
import { useSecurityStore } from '../../stores/useSecurityStore';

export const LegacyArchiveRetirementCard: React.FC = () => {
  const { language } = useAppStore();
  const hasDpapiWrap = useSecurityStore((s) => s.hasDpapiWrap);
  const ui = (zh: string, en: string) => (language === 'en-US' ? en : zh);

  const [pages, setPages] = useState<LegacySourcePage[]>([]);
  const [selected, setSelected] = useState('');
  const [password, setPassword] = useState('');
  const [confirmation, setConfirmation] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [message, setMessage] = useState('');
  const [loaded, setLoaded] = useState(false);
  const [offset, setOffset] = useState(0);
  const requestVersion = useRef(0);
  const mounted = useRef(false);

  const refresh = async (clearError = true) => {
    const version = ++requestVersion.current;
    try {
      const result = await archiveRetirement.list(offset);
      if (!mounted.current || version !== requestVersion.current) return;
      setPages(result);
      setSelected((old) => (result.some((p) => p.page_uuid === old) ? old : (result[0]?.page_uuid || '')));
      setLoaded(true);
      if (clearError) setError('');
    } catch {
      if (mounted.current && version === requestVersion.current) {
        setError((previous) =>
          !clearError && previous
            ? previous
            : ui('旧网页源目录状态无法验证，请解锁后重试。', 'Unable to verify legacy archive source folders; unlock vault and retry.')
        );
      }
    }
  };

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      requestVersion.current++;
    };
  }, []);

  useEffect(() => {
    void refresh();
  }, [offset]);

  const retire = async () => {
    if (busy || !selected || (!hasDpapiWrap && !password) || confirmation !== '移除旧网页源目录') return;
    setBusy(true);
    setError('');
    setMessage('');
    try {
      const result = await archiveRetirement.retire(selected, hasDpapiWrap ? '' : password, confirmation);
      if (mounted.current) {
        setMessage(
          ui(
            `指定旧网页源目录已移除。已验证的加密恢复配对保存在 ${result.recovery_relative_path}/；口令与恢复密钥以生成该副本时为准。`,
            `Selected legacy archive source removed. Verified encrypted recovery pair saved in ${result.recovery_relative_path}/. Passphrase and recovery key match the time of creation.`
          )
        );
      }
    } catch (err) {
      if (mounted.current) {
        setError(
          typeof err === 'string'
            ? err
            : ui('退役尚未完成，请检查原目录和加密恢复配对后重试。', 'Retirement incomplete; verify the source directory and recovery pair.')
        );
      }
    } finally {
      if (mounted.current) {
        setPassword('');
        setConfirmation('');
        await refresh(false);
        if (mounted.current) setBusy(false);
      }
    }
  };

  const current = pages.find((p) => p.page_uuid === selected);

  return (
    <section className="rounded-xl border border-amber-200 dark:border-amber-900 bg-amber-50/70 dark:bg-amber-950/20 p-3 text-xs space-y-2">
      <h3 className="font-semibold text-amber-900 dark:text-amber-200">
        {ui('旧网页源目录退役', 'Retire legacy webpage archive folders')}
      </h3>
      <p className="text-slate-700 dark:text-slate-300">
        {ui(
          '旧版网页截图、正文和附件的原始文件可能仍为明文。每次仅处理你选定的目录：核对加密内容，生成并验证独立恢复配对，然后移除原目录。',
          'Legacy screenshots, body snapshots, and attachments may still be unencrypted on disk. Each action handles only the selected folder after verifying an encrypted recovery pair.'
        )}
      </p>
      <p className="text-slate-600 dark:text-slate-400">
        {ui(
          '每次保存一份完整的当前保险库，可能需要较多时间和磁盘空间。文件移除不可撤销，不保证磁盘物理擦除。恢复配对保留在 archive-retired/，不属于 ZIP 备份列表。未知目录、链接和变化的内容会保留。',
          'Each step stores an encrypted recovery pair under archive-retired/ (outside ZIP backup rotation). Removal is irreversible and unexpected/symlinked files are preserved.'
        )}
      </p>
      {loaded && pages.length === 0 && (
        <p className="text-slate-600 dark:text-slate-400">
          {offset === 0
            ? ui('未发现可列出的旧网页 UUID 源目录。', 'No legacy webpage UUID directories found.')
            : ui('本页没有更多旧网页目录。', 'No more legacy webpage directories on this page.')}
        </p>
      )}
      {pages.length > 0 && (
        <>
          <label className="flex flex-col gap-1">
            <span>{ui('选择旧网页目录', 'Select legacy directory')}</span>
            <select
              value={selected}
              disabled={busy}
              onChange={(e) => {
                setSelected(e.target.value);
                setPassword('');
                setConfirmation('');
                setMessage('');
              }}
              className="p-2 rounded border border-slate-300 dark:border-slate-700 bg-white dark:bg-slate-900"
            >
              {pages.map((p) => (
                <option key={p.page_uuid} value={p.page_uuid}>
                  {p.page_uuid}
                  {p.resuming ? ui('（继续上次退役）', ' (resume)') : ''}
                </option>
              ))}
            </select>
          </label>
          {current?.can_retire ? (
            <>
              <p>
                {ui(
                  `剩余源文件 ${current.file_count} 个，${(current.total_bytes / 1024).toFixed(1)} KiB。加密副本与迁移凭据已通过当前检查，执行前将再次验证。`,
                  `${current.file_count} source file(s), ${(current.total_bytes / 1024).toFixed(1)} KiB. Encrypted archive copy verified; re-verified before removal.`
                )}
              </p>
              {!hasDpapiWrap && (
                <input
                  aria-label={ui('旧网页退役当前口令', 'Current vault passphrase')}
                  type="password"
                  autoComplete="off"
                  value={password}
                  disabled={busy}
                  onChange={(e) => setPassword(e.target.value)}
                  placeholder={ui('当前保险库口令（重新验证）', 'Current vault passphrase (re-verify)')}
                  className="p-2 w-full rounded border border-slate-300 dark:border-slate-700 bg-white dark:bg-slate-900"
                />
              )}
              <input
                aria-label={ui('旧网页退役确认文字', 'Retirement confirmation phrase')}
                value={confirmation}
                disabled={busy}
                onChange={(e) => setConfirmation(e.target.value)}
                placeholder={ui('输入：移除旧网页源目录', 'Type exact phrase: 移除旧网页源目录')}
                className="p-2 w-full rounded border border-slate-300 dark:border-slate-700 bg-white dark:bg-slate-900"
              />
              <button
                type="button"
                disabled={busy || (!hasDpapiWrap && !password) || confirmation !== '移除旧网页源目录'}
                onClick={() => void retire()}
                className="px-3 py-2 rounded bg-amber-800 text-white disabled:opacity-50 cursor-pointer"
              >
                {busy
                  ? ui('正在备份、验证并移除…', 'Backing up, verifying & removing...')
                  : ui('验证恢复副本并移除指定旧网页目录', 'Verify recovery pair & remove folder')}
              </button>
            </>
          ) : (
            <p role="alert">{current?.reason}</p>
          )}
        </>
      )}
      {message && <p role="status">{message}</p>}
      {error && <p role="alert" className="text-rose-700 dark:text-rose-300">{error}</p>}
      <div className="flex gap-3">
        <button
          type="button"
          disabled={busy || offset === 0}
          onClick={() => {
            setOffset(Math.max(0, offset - 20));
            setPassword('');
            setConfirmation('');
          }}
          className="underline disabled:opacity-50 cursor-pointer"
        >
          {ui('上一页', 'Previous')}
        </button>
        <button
          type="button"
          disabled={busy || pages.length < 20}
          onClick={() => {
            setOffset(offset + 20);
            setPassword('');
            setConfirmation('');
          }}
          className="underline disabled:opacity-50 cursor-pointer"
        >
          {ui('下一页', 'Next')}
        </button>
      </div>
      <button type="button" disabled={busy} onClick={() => void refresh()} className="underline disabled:opacity-50 cursor-pointer">
        {ui('重新检查', 'Recheck')}
      </button>
    </section>
  );
};
