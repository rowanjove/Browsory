import React, { useRef, useState } from 'react';
import {
  Shield,
  Lock,
  KeyRound,
  AlertCircle,
  AlertTriangle,
  X,
  Copy,
  Check,
  FileText,
  Database,
  LockKeyhole,
  CheckCircle2,
  Loader2,
  Eye,
  EyeOff,
} from 'lucide-react';
import { useSecurityStore } from '../../stores/useSecurityStore';
import { useAppStore } from '../../stores/useAppStore';
import { useDialogFocus } from '../../hooks/useDialogFocus';

export const SecuritySettingsCard: React.FC = () => {
  const { showToast, t, language } = useAppStore();
  const ui = (zh: string, en: string) => (language === 'en-US' ? en : zh);
  const {
    vaultStatus,
    hasDpapiWrap,
    pinEnabled,
    pinMigrationNeeded,
    autoLockMinutes,
    lockOnMinimize,
    lockOnSleep,
    nativeSleepLockSupported,
    updateOptions,
    setOrChangePin,
    disablePin,
    lockVault,
    changeVaultPassword,
  } = useSecurityStore();

  // PIN modal state
  const [pinModalMode, setPinModalMode] = useState<'set' | 'change' | 'disable' | null>(null);
  const [oldPinInput, setOldPinInput] = useState('');
  const [newPinInput, setNewPinInput] = useState('');
  const [confirmPinInput, setConfirmPinInput] = useState('');
  const [pinModalError, setPinModalError] = useState('');
  const [isSubmittingPin, setIsSubmittingPin] = useState(false);
  const [generatedRecoveryKey, setGeneratedRecoveryKey] = useState<string | null>(null);
  const [hasConfirmedPinRecoveryKey, setHasConfirmedPinRecoveryKey] = useState(false);
  const [isCopied, setIsCopied] = useState(false);

  // Vault Password Change Modal State
  const [showVaultPasswordModal, setShowVaultPasswordModal] = useState(false);
  const [vaultOldPassword, setVaultOldPassword] = useState('');
  const [vaultNewPassword, setVaultNewPassword] = useState('');
  const [vaultConfirmPassword, setVaultConfirmPassword] = useState('');
  const [vaultModalError, setVaultModalError] = useState('');
  const [isSubmittingVaultPassword, setIsSubmittingVaultPassword] = useState(false);
  const [showVaultPasswords, setShowVaultPasswords] = useState(false);

  const vaultOldPwdRef = useRef<HTMLInputElement>(null);
  const vaultPwdDialogRef = useDialogFocus<HTMLDivElement>({
    isOpen: showVaultPasswordModal,
    onClose: () => {
      if (!isSubmittingVaultPassword) setShowVaultPasswordModal(false);
    },
    initialFocusRef: vaultOldPwdRef,
  });

  const handleClosePinModal = () => {
    if (isSubmittingPin) return;
    setPinModalMode(null);
    setPinModalError('');
  };

  const pinDialogRef = useDialogFocus<HTMLDivElement>({
    isOpen: Boolean(pinModalMode),
    onClose: handleClosePinModal,
  });

  const pinRecoveryDialogRef = useDialogFocus<HTMLDivElement>({
    isOpen: Boolean(generatedRecoveryKey),
    onClose: () => {
      if (hasConfirmedPinRecoveryKey) {
        setGeneratedRecoveryKey(null);
        setHasConfirmedPinRecoveryKey(false);
      }
    },
  });

  const handleOpenPinModal = (mode: 'set' | 'change' | 'disable') => {
    setPinModalMode(mode);
    setOldPinInput('');
    setNewPinInput('');
    setConfirmPinInput('');
    setPinModalError('');
  };

  const handlePinSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setPinModalError('');

    if (pinModalMode === 'disable') {
      if (!oldPinInput) {
        setPinModalError(ui('请输入当前 PIN 码以确认停用', 'Enter your current PIN to disable'));
        return;
      }
      setIsSubmittingPin(true);
      try {
        await disablePin(oldPinInput);
        showToast(ui('界面 PIN 锁已停用', 'Screen PIN lock disabled'), 'info');
        setPinModalMode(null);
        setPinModalError('');
      } catch (err: any) {
        setPinModalError(typeof err === 'string' ? err : ui('停用失败，当前 PIN 错误', 'Failed to disable PIN; check your current PIN'));
      } finally {
        setIsSubmittingPin(false);
      }
      return;
    }

    if (pinModalMode === 'change' && !oldPinInput) {
      setPinModalError(ui('请输入原 PIN 码', 'Enter your current PIN'));
      return;
    }

    if (!/^\d{4}$/.test(newPinInput)) {
      setPinModalError(ui('新 PIN 码必须为 4 位纯数字', 'New PIN must be 4 numeric digits'));
      return;
    }

    if (newPinInput !== confirmPinInput) {
      setPinModalError(ui('两次输入的新 PIN 码不一致', 'The two PIN entries do not match'));
      return;
    }

    setIsSubmittingPin(true);
    try {
      const recKey = await setOrChangePin(pinModalMode === 'change' ? oldPinInput : null, newPinInput);
      showToast(
        pinModalMode === 'change' ? ui('PIN 码修改成功', 'PIN updated') : ui('界面 PIN 锁已启用', 'Screen PIN lock enabled'),
        'success'
      );
      setPinModalMode(null);
      setPinModalError('');
      if (recKey) {
        setHasConfirmedPinRecoveryKey(false);
        setIsCopied(false);
        setGeneratedRecoveryKey(recKey);
      }
    } catch (err: any) {
      setPinModalError(typeof err === 'string' ? err : ui('设置失败', 'Failed to update PIN'));
    } finally {
      setIsSubmittingPin(false);
    }
  };

  // Vault Password Change Handler
  const handleVaultPasswordSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setVaultModalError('');

    if (!vaultOldPassword) {
      setVaultModalError(ui('请输入当前数据库口令', 'Enter your current vault passphrase'));
      return;
    }
    if (Array.from(vaultNewPassword.trim()).length < 12) {
      setVaultModalError(ui('新口令长度必须至少为 12 个字符', 'New passphrase must be at least 12 characters'));
      return;
    }
    if (vaultNewPassword !== vaultConfirmPassword) {
      setVaultModalError(ui('两次输入的新口令不一致', 'The two passphrases do not match'));
      return;
    }

    setIsSubmittingVaultPassword(true);
    try {
      await changeVaultPassword(vaultOldPassword, vaultNewPassword);
      showToast(ui('数据库主加密口令已更新（主密钥已重新封装）', 'Vault passphrase updated (master key re-wrapped)'), 'success');
      setShowVaultPasswordModal(false);
      setVaultOldPassword('');
      setVaultNewPassword('');
      setVaultConfirmPassword('');
    } catch (err: any) {
      setVaultModalError(typeof err === 'string' ? err : err?.message || ui('修改口令失败', 'Failed to change passphrase'));
    } finally {
      setIsSubmittingVaultPassword(false);
    }
  };

  return (
    <div className="bg-white dark:bg-slate-800 p-5 rounded-2xl border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-6">
      {/* Header */}
      <div className="flex items-center justify-between pb-3 border-b border-slate-100 dark:border-slate-800">
        <div className="flex items-center gap-2.5">
          <Shield className="w-5 h-5 text-blue-600 dark:text-blue-400" />
          <span className="font-semibold text-slate-800 dark:text-slate-100 text-sm">
            {ui('数据库保险库与安全凭据', 'Database vault & credentials')}
          </span>
        </div>
      </div>

      {/* 1. Database Vault Section */}
      <div className="p-4 bg-blue-50/40 dark:bg-blue-950/20 rounded-xl border border-blue-100 dark:border-blue-900/40 flex flex-col gap-3">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Database className="w-4 h-4 text-blue-600 dark:text-blue-400" />
            <span className="font-semibold text-xs text-slate-900 dark:text-slate-100">
              {ui('数据保险库 (Database Vault)', 'Database Vault')}
            </span>
          </div>
          <span className="text-[11px] px-2.5 py-0.5 rounded-full font-medium bg-emerald-100 text-emerald-800 dark:bg-emerald-950/80 dark:text-emerald-300">
            {vaultStatus === 'unlocked'
              ? hasDpapiWrap
                ? ui('后台静态加密已启用 (SQLCipher · DPAPI)', 'Passive encryption active (SQLCipher · DPAPI)')
                : ui('静态加密已激活 (SQLCipher v4)', 'At-rest encryption active (SQLCipher v4)')
              : ui('保险库未解锁', 'Vault locked')}
          </span>
        </div>

        <div className="text-xs text-slate-600 dark:text-slate-400 leading-relaxed">
          {hasDpapiWrap
            ? ui(
                'archive.db 在后台全程采用 SQLCipher (AES-256-CBC) 静态加密存储，主密钥由本机 Windows DPAPI 系统密钥自动封装，他人直接拷贝数据库文件无法读取明文，且启动时无需反复输入口令。',
                'archive.db is silently encrypted at rest using SQLCipher (AES-256-CBC) with a Windows DPAPI-protected master key. Copied files cannot be read as plaintext, and no startup passphrase prompt is required.'
              )
            : ui(
                'archive.db 采用 256 位 AES-CBC 页面级物理加密存储，主密钥由用户口令通过 Argon2id 派生密钥进行封装，并保存在独立的 vault.envelope.json 信封中。',
                'archive.db is stored with 256-bit AES-CBC page encryption. The master key is wrapped with an Argon2id key derived from your passphrase in vault.envelope.json.'
              )}
        </div>

        <div className="flex flex-wrap items-center justify-between gap-2 pt-2 border-t border-blue-100/60 dark:border-blue-900/30">
          <div className="flex items-center gap-1.5 text-xs text-slate-500">
            <LockKeyhole className="w-3.5 h-3.5 text-blue-500" />
            <span>
              {hasDpapiWrap
                ? ui('算法规范: AES-256-CBC · Windows DPAPI 自动解封', 'Crypto: AES-256-CBC · Windows DPAPI auto-unwrap')
                : ui('算法规范: AES-256-CBC · Argon2id (64MB) · AAD 绑定', 'Crypto: AES-256-CBC · Argon2id (64MB) · AAD bound')}
            </span>
          </div>

          <div className="flex items-center gap-2">
            {!hasDpapiWrap && (
              <button
                type="button"
                onClick={() => {
                  setVaultOldPassword('');
                  setVaultNewPassword('');
                  setVaultConfirmPassword('');
                  setVaultModalError('');
                  setShowVaultPasswordModal(true);
                }}
                className="px-3 py-1.5 rounded-lg bg-white dark:bg-slate-800 hover:bg-slate-50 dark:hover:bg-slate-700 border border-slate-200 dark:border-slate-700 text-slate-700 dark:text-slate-200 text-xs font-medium transition shadow-xs cursor-pointer"
              >
                {ui('修改数据库主口令', 'Change vault passphrase')}
              </button>
            )}
            <button
              type="button"
              onClick={async () => {
                if (hasDpapiWrap && !pinEnabled) {
                  handleOpenPinModal('set');
                  showToast(ui('请先设置 4 位 PIN 码以启用主动锁定保护', 'Set a 4-digit PIN first to lock manually'), 'info');
                  return;
                }
                await lockVault();
                showToast(ui('已关闭数据库连接并锁定保险库', 'Closed database connection and locked vault'), 'info');
              }}
              className="px-3 py-1.5 rounded-lg bg-rose-50 hover:bg-rose-100 dark:bg-rose-950/50 dark:hover:bg-rose-900/60 text-rose-600 dark:text-rose-400 border border-rose-200/80 dark:border-rose-800/60 text-xs font-medium transition cursor-pointer"
            >
              {ui('立即锁定保险库', 'Lock vault now')}
            </button>
          </div>
        </div>
      </div>

      {/* 2. UI Quick PIN Lock Section */}
      <div className="p-4 bg-slate-50/70 dark:bg-slate-900/50 rounded-xl border border-slate-200/80 dark:border-slate-800 flex flex-col gap-3">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Lock className="w-4 h-4 text-slate-600 dark:text-slate-300" />
            <span className="font-semibold text-xs text-slate-800 dark:text-slate-200">
              {ui('应用界面 4 位 PIN 保护（可选）', 'App 4-digit PIN Lock (Optional)')}
            </span>
          </div>
          <span
            className={`text-[11px] px-2.5 py-0.5 rounded-full font-medium ${
              pinEnabled
                ? 'bg-emerald-50 text-emerald-700 dark:bg-emerald-950/50 dark:text-emerald-300'
                : 'bg-slate-100 text-slate-500 dark:bg-slate-700 dark:text-slate-400'
            }`}
          >
            {pinEnabled ? ui('PIN 锁已启用', 'PIN enabled') : ui('未加锁', 'Disabled')}
          </span>
        </div>

        <div className="text-xs text-slate-500 leading-relaxed">
          {ui(
            '开启后仅在需要防旁窥或启动校验时输入 4 位纯数字 PIN；数据库文件本身始终保持静态加密。',
            'When enabled, requires a 4-digit PIN on startup wake or manual lock. Your database remains encrypted at rest regardless.'
          )}
        </div>

        {pinEnabled && pinMigrationNeeded && (
          <div role="status" className="p-3 rounded-xl bg-amber-50 dark:bg-amber-950/30 border border-amber-200 dark:border-amber-800/60 flex items-center justify-between gap-3 text-xs text-amber-800 dark:text-amber-300">
            <div className="flex items-start gap-2">
              <AlertTriangle className="w-4 h-4 shrink-0 mt-0.5 text-amber-600 dark:text-amber-400" />
              <span>
                {ui(
                  '检测到当前 PIN 仍采用旧版存储格式，建议更新一次 PIN 以升级为 Argon2id 加密信封存储。',
                  'Legacy PIN storage detected. Update your PIN once to migrate it to Argon2id envelope protection.'
                )}
              </span>
            </div>
            <button
              type="button"
              onClick={() => handleOpenPinModal('change')}
              className="px-3 py-1 rounded-lg bg-amber-600 hover:bg-amber-700 text-white font-medium shrink-0 cursor-pointer"
            >
              {ui('升级 PIN 存储', 'Upgrade PIN')}
            </button>
          </div>
        )}

        <div className="flex items-center justify-between pt-1">
          <span className="text-xs text-slate-400">
            {pinEnabled
              ? ui('已启用自动锁屏保护', 'Auto-lock screen protection is active')
              : ui('离开电脑时可通过 PIN 码遮挡界面', 'Protect the open interface with a quick PIN')}
          </span>
          <div className="flex items-center gap-2">
            {pinEnabled ? (
              <>
                <button
                  type="button"
                  onClick={() => handleOpenPinModal('change')}
                  className="px-2.5 py-1.5 rounded-lg bg-slate-200/80 dark:bg-slate-800 hover:bg-slate-300/80 dark:hover:bg-slate-700 text-slate-700 dark:text-slate-200 text-xs font-medium transition cursor-pointer"
                >
                  {ui('修改 PIN', 'Change PIN')}
                </button>
                <button
                  type="button"
                  onClick={() => handleOpenPinModal('disable')}
                  className="px-2.5 py-1.5 rounded-lg bg-rose-50 hover:bg-rose-100 dark:bg-rose-950/40 dark:hover:bg-rose-900/50 text-rose-600 dark:text-rose-400 text-xs font-medium transition cursor-pointer"
                >
                  {ui('停用', 'Disable')}
                </button>
              </>
            ) : (
              <button
                type="button"
                onClick={() => handleOpenPinModal('set')}
                className="px-3 py-1.5 rounded-lg bg-blue-600 hover:bg-blue-700 text-white text-xs font-medium shadow-xs transition cursor-pointer"
              >
                {ui('启用界面 PIN 锁', 'Enable screen PIN')}
              </button>
            )}
          </div>
        </div>

        {/* Auto-Lock Options */}
        {pinEnabled && (
          <div className="grid grid-cols-1 sm:grid-cols-3 gap-3 pt-3 border-t border-slate-200/60 dark:border-slate-800/60 text-xs">
            <div className="flex flex-col gap-1">
              <label className="text-slate-600 dark:text-slate-400 font-medium text-xs">{ui('闲置自动锁定', 'Idle auto-lock')}</label>
              <select
                value={autoLockMinutes}
                onChange={(e) =>
                  updateOptions(Number(e.target.value), lockOnMinimize, lockOnSleep)
                }
                className="px-2 py-1.5 rounded-lg border border-slate-300 dark:border-slate-700 bg-white dark:bg-slate-900 text-slate-800 dark:text-slate-200 text-xs"
              >
                <option value="0">{ui('从不 (仅手动锁定)', 'Never (manual only)')}</option>
                <option value="5">{ui('5 分钟', '5 minutes')}</option>
                <option value="15">{ui('15 分钟 (推荐)', '15 minutes (recommended)')}</option>
                <option value="30">{ui('30 分钟', '30 minutes')}</option>
                <option value="60">{ui('1 小时', '1 hour')}</option>
              </select>
            </div>

            <div className="flex items-center gap-2 pt-4 sm:pt-4">
              <input
                type="checkbox"
                id="lock_minimize"
                checked={lockOnMinimize}
                onChange={(e) =>
                  updateOptions(autoLockMinutes, e.target.checked, lockOnSleep)
                }
                className="w-4 h-4 rounded text-blue-600 focus:ring-blue-500 border-slate-300 dark:border-slate-700"
              />
              <label htmlFor="lock_minimize" className="text-slate-700 dark:text-slate-300 select-none text-xs">
                {ui('应用隐藏/最小化后锁定（平台事件）', 'Lock on hide/minimize (platform event)')}
              </label>
            </div>
          </div>
        )}
        <div className="mt-4 border-t border-slate-200/60 pt-4 dark:border-slate-800/60">
          <label className="flex items-center gap-2 text-xs text-slate-700 dark:text-slate-300">
            <input
              type="checkbox"
              checked={lockOnSleep}
              disabled={!nativeSleepLockSupported}
              onChange={(event) => updateOptions(autoLockMinutes, lockOnMinimize, event.target.checked)}
              className="h-4 w-4 rounded border-slate-300 text-blue-600 focus:ring-blue-500 disabled:opacity-40"
            />
            <span>{t('security.nativeSleepLabel')}</span>
          </label>
          <p className="mt-2 text-xs leading-relaxed text-slate-500 dark:text-slate-400">
            {t(nativeSleepLockSupported ? 'security.nativeSleepDescription' : 'security.nativeSleepUnavailable')}
          </p>
        </div>
      </div>

      {/* Vault Password Change Modal */}
      {showVaultPasswordModal && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/60 backdrop-blur-xs p-4"
          onClick={() => {
            if (!isSubmittingVaultPassword) setShowVaultPasswordModal(false);
          }}
          role="presentation"
        >
          <div
            ref={vaultPwdDialogRef}
            role="dialog"
            aria-modal="true"
            aria-labelledby="vault-password-modal-title"
            tabIndex={-1}
            onClick={(e) => e.stopPropagation()}
            className="w-full max-w-sm p-6 bg-white dark:bg-slate-900 border border-slate-200 dark:border-slate-800 rounded-3xl shadow-2xl animate-in zoom-in-95 flex flex-col gap-4"
          >
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <Database className="w-5 h-5 text-blue-600" />
                <h3 id="vault-password-modal-title" className="font-bold text-sm text-slate-900 dark:text-slate-100">
                  {ui('修改数据库主加密口令', 'Change vault passphrase')}
                </h3>
              </div>
              <button
                type="button"
                onClick={() => {
                  if (!isSubmittingVaultPassword) setShowVaultPasswordModal(false);
                }}
                disabled={isSubmittingVaultPassword}
                aria-label={ui('关闭修改口令窗口', 'Close passphrase modal')}
                className="p-1 text-slate-400 hover:text-slate-600 dark:hover:text-slate-200 cursor-pointer disabled:opacity-40"
              >
                <X className="w-4 h-4" />
              </button>
            </div>

            <p className="text-xs text-slate-500 leading-relaxed">
              {ui(
                '修改口令将使用 Argon2id 重新派生密钥并重封装主密钥信封 (vault.envelope.json)，无需全库重新加密。',
                'Re-wraps your master key in vault.envelope.json via Argon2id without re-encrypting the entire database.'
              )}
            </p>

            <form onSubmit={handleVaultPasswordSubmit} className="flex flex-col gap-3 text-xs">
              <div className="flex flex-col gap-1">
                <label className="text-slate-600 dark:text-slate-400 font-medium">{ui('当前主口令', 'Current passphrase')}</label>
                <div className="relative">
                  <input
                    ref={vaultOldPwdRef}
                    type={showVaultPasswords ? 'text' : 'password'}
                    value={vaultOldPassword}
                    onChange={(e) => setVaultOldPassword(e.target.value)}
                    placeholder={ui('输入当前数据库口令', 'Enter current vault passphrase')}
                    className="w-full px-3 py-2 rounded-xl border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-800 text-slate-800 dark:text-slate-100 pr-9"
                  />
                  <button
                    type="button"
                    onClick={() => setShowVaultPasswords((p) => !p)}
                    aria-label={showVaultPasswords ? ui('隐藏口令', 'Hide passphrase') : ui('显示口令', 'Show passphrase')}
                    className="absolute right-2.5 top-2 text-slate-400 hover:text-slate-600 cursor-pointer"
                  >
                    {showVaultPasswords ? <EyeOff className="w-3.5 h-3.5" /> : <Eye className="w-3.5 h-3.5" />}
                  </button>
                </div>
              </div>

              <div className="flex flex-col gap-1">
                <label className="text-slate-600 dark:text-slate-400 font-medium">{ui('新主口令 (至少 12 个字符)', 'New passphrase (at least 12 characters)')}</label>
                <input
                  type={showVaultPasswords ? 'text' : 'password'}
                  value={vaultNewPassword}
                  onChange={(e) => setVaultNewPassword(e.target.value)}
                  placeholder={ui('输入新主加密口令', 'Enter new vault passphrase')}
                  className="px-3 py-2 rounded-xl border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-800 text-slate-800 dark:text-slate-100"
                />
              </div>

              <div className="flex flex-col gap-1">
                <label className="text-slate-600 dark:text-slate-400 font-medium">{ui('确认新主口令', 'Confirm new passphrase')}</label>
                <input
                  type={showVaultPasswords ? 'text' : 'password'}
                  value={vaultConfirmPassword}
                  onChange={(e) => setVaultConfirmPassword(e.target.value)}
                  placeholder={ui('再次确认新口令', 'Re-enter new passphrase')}
                  className="px-3 py-2 rounded-xl border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-800 text-slate-800 dark:text-slate-100"
                />
              </div>

              {vaultModalError && (
                <div role="alert" className="text-rose-500 text-xs flex items-center gap-1.5 mt-1">
                  <AlertCircle className="w-3.5 h-3.5 shrink-0" />
                  <span>{vaultModalError}</span>
                </div>
              )}

              <div className="flex justify-end gap-2 mt-3">
                <button
                  type="button"
                  onClick={() => setShowVaultPasswordModal(false)}
                  disabled={isSubmittingVaultPassword}
                  className="px-3 py-1.5 rounded-xl border border-slate-300 dark:border-slate-700 text-slate-600 dark:text-slate-300 hover:bg-slate-100 dark:hover:bg-slate-800 cursor-pointer disabled:opacity-40"
                >
                  {t('export.cancel')}
                </button>
                <button
                  type="submit"
                  disabled={isSubmittingVaultPassword}
                  className="px-4 py-1.5 rounded-xl bg-blue-600 hover:bg-blue-700 text-white font-medium flex items-center gap-1.5 disabled:opacity-50 cursor-pointer"
                >
                  {isSubmittingVaultPassword ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <CheckCircle2 className="w-3.5 h-3.5" />}
                  <span>{isSubmittingVaultPassword ? ui('正在更新信封...', 'Updating envelope...') : ui('确认更新', 'Confirm update')}</span>
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      {/* PIN Setup / Change Modal */}
      {pinModalMode && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/50 backdrop-blur-xs p-4"
          onClick={handleClosePinModal}
          role="presentation"
        >
          <div
            ref={pinDialogRef}
            role="dialog"
            aria-modal="true"
            aria-labelledby="pin-modal-title"
            tabIndex={-1}
            onClick={(e) => e.stopPropagation()}
            className="w-full max-w-sm p-6 bg-white dark:bg-slate-900 border border-slate-200 dark:border-slate-800 rounded-3xl shadow-2xl animate-in zoom-in-95"
          >
            <div className="flex items-center justify-between mb-4">
              <div className="flex items-center gap-2">
                <KeyRound className="w-5 h-5 text-blue-600" />
                <h3 id="pin-modal-title" className="font-bold text-sm text-slate-900 dark:text-slate-100">
                  {pinModalMode === 'set'
                    ? ui('设置应用界面锁 PIN', 'Set screen lock PIN')
                    : pinModalMode === 'change'
                    ? ui('修改应用界面锁 PIN', 'Change screen lock PIN')
                    : ui('停用应用界面锁', 'Disable screen lock PIN')}
                </h3>
              </div>
              <button
                type="button"
                onClick={handleClosePinModal}
                disabled={isSubmittingPin}
                aria-label={ui('关闭 PIN 设置窗口', 'Close PIN modal')}
                className="p-1 text-slate-400 hover:text-slate-600 cursor-pointer disabled:opacity-40"
              >
                <X className="w-4 h-4" />
              </button>
            </div>

            <form onSubmit={handlePinSubmit} className="flex flex-col gap-3">
              {pinModalMode !== 'set' && (
                <div className="flex flex-col gap-1">
                  <label className="text-slate-500 font-medium text-xs">{ui('当前 PIN 码', 'Current PIN')}</label>
                  <input
                    type="password"
                    maxLength={6}
                    inputMode="numeric"
                    autoFocus
                    placeholder={ui('输入当前 4 位 PIN', 'Enter current 4-digit PIN')}
                    value={oldPinInput}
                    onChange={(e) => setOldPinInput(e.target.value.replace(/\D/g, '').slice(0, 6))}
                    className="px-3 py-2 rounded-xl border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-800 font-mono tracking-widest text-center text-xs"
                  />
                </div>
              )}

              {pinModalMode !== 'disable' && (
                <>
                  <div className="flex flex-col gap-1">
                    <label className="text-slate-500 font-medium text-xs">{ui('新 PIN 码 (4 位纯数字)', 'New PIN (4 numeric digits)')}</label>
                    <input
                      type="password"
                      maxLength={4}
                      inputMode="numeric"
                      autoFocus={pinModalMode === 'set'}
                      placeholder={ui('输入 4 位纯数字', '4 digits')}
                      value={newPinInput}
                      onChange={(e) => setNewPinInput(e.target.value.replace(/\D/g, '').slice(0, 4))}
                      className="px-3 py-2 rounded-xl border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-800 font-mono tracking-widest text-center text-xs"
                    />
                  </div>

                  <div className="flex flex-col gap-1">
                    <label className="text-slate-500 font-medium text-xs">{ui('再次确认新 PIN 码', 'Confirm new PIN')}</label>
                    <input
                      type="password"
                      maxLength={4}
                      inputMode="numeric"
                      placeholder={ui('再次输入 4 位新 PIN', 'Re-enter 4-digit PIN')}
                      value={confirmPinInput}
                      onChange={(e) => setConfirmPinInput(e.target.value.replace(/\D/g, '').slice(0, 4))}
                      className="px-3 py-2 rounded-xl border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-800 font-mono tracking-widest text-center text-xs"
                    />
                  </div>
                </>
              )}

              {pinModalError && (
                <div role="alert" className="text-rose-500 text-xs flex items-center gap-1.5 mt-1">
                  <AlertCircle className="w-3.5 h-3.5 flex-shrink-0" />
                  <span>{pinModalError}</span>
                </div>
              )}

              <div className="flex justify-end gap-2 mt-4">
                <button
                  type="button"
                  onClick={handleClosePinModal}
                  disabled={isSubmittingPin}
                  className="px-3 py-1.5 rounded-xl border border-slate-300 dark:border-slate-700 text-slate-600 dark:text-slate-300 hover:bg-slate-100 dark:hover:bg-slate-800 text-xs cursor-pointer disabled:opacity-40"
                >
                  {t('export.cancel')}
                </button>
                <button
                  type="submit"
                  disabled={isSubmittingPin}
                  className="px-4 py-1.5 rounded-xl bg-blue-600 hover:bg-blue-700 text-white font-medium text-xs cursor-pointer disabled:opacity-50"
                >
                  {isSubmittingPin ? ui('处理中...', 'Saving...') : ui('确认', 'Confirm')}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      {/* Recovery Key Presentation Modal (For PIN reset) */}
      {generatedRecoveryKey && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/60 backdrop-blur-sm p-4 animate-in fade-in"
          role="presentation"
        >
          <div
            ref={pinRecoveryDialogRef}
            role="dialog"
            aria-modal="true"
            aria-labelledby="pin-recovery-created-title"
            tabIndex={-1}
            className="w-full max-w-md bg-white dark:bg-slate-800 rounded-3xl border border-slate-200 dark:border-slate-700 shadow-2xl p-6 flex flex-col gap-4"
          >
            <div className="flex items-center gap-3">
              <div className="w-10 h-10 rounded-xl bg-amber-50 dark:bg-amber-950/60 border border-amber-200 dark:border-amber-800/60 flex items-center justify-center text-amber-600 dark:text-amber-400 shadow-inner">
                <KeyRound className="w-5 h-5" />
              </div>
              <div>
                <h3 id="pin-recovery-created-title" className="font-bold text-base text-slate-900 dark:text-slate-100">
                  {ui('安全恢复密钥 (Recovery Key)', 'PIN Recovery Key')}
                </h3>
                <p className="text-xs text-slate-500">
                  {ui('用于忘记 PIN 时重置应用界面锁凭据', 'Use this key to reset the screen PIN if forgotten')}
                </p>
              </div>
            </div>

            <p className="text-xs text-slate-600 dark:text-slate-300 leading-relaxed">
              {ui(
                '请妥善保管此恢复密钥。若遗忘界面 PIN 码，可在锁屏界面通过此凭据重置 PIN。关闭后将不再完整显示。',
                'Store this key safely. You can reset a forgotten screen PIN with it on the lock screen. It will not be shown again once closed.'
              )}
            </p>

            <div className="p-3.5 bg-slate-100 dark:bg-slate-900 rounded-xl border border-slate-200 dark:border-slate-700/80 font-mono text-xs text-slate-800 dark:text-slate-100 break-all select-all flex items-center justify-between gap-2">
              <span className="font-semibold tracking-wider">{generatedRecoveryKey}</span>
              <button
                type="button"
                onClick={async () => {
                  try {
                    await navigator.clipboard.writeText(generatedRecoveryKey);
                    setIsCopied(true);
                    setTimeout(() => setIsCopied(false), 2000);
                    showToast(ui('恢复密钥已复制到剪贴板', 'Recovery key copied to clipboard'), 'info');
                  } catch {
                    showToast(ui('复制到剪贴板失败，请手动复制或保存为文本文件', 'Clipboard failed; copy manually or save as text file'), 'error');
                  }
                }}
                aria-label={ui('复制恢复密钥', 'Copy recovery key')}
                className="p-1.5 rounded-lg hover:bg-slate-200 dark:hover:bg-slate-800 text-slate-500 hover:text-slate-700 dark:hover:text-slate-200 transition cursor-pointer"
                title={ui('复制', 'Copy')}
              >
                {isCopied ? <Check className="w-4 h-4 text-emerald-500" /> : <Copy className="w-4 h-4" />}
              </button>
            </div>

            <label className="flex items-start gap-2 text-xs text-slate-700 dark:text-slate-300 cursor-pointer select-none">
              <input
                type="checkbox"
                checked={hasConfirmedPinRecoveryKey}
                onChange={(e) => setHasConfirmedPinRecoveryKey(e.target.checked)}
                className="mt-0.5 w-4 h-4 rounded text-blue-600 focus:ring-blue-500 border-slate-300 dark:border-slate-700"
              />
              <span>
                {ui('我已妥善保存此 PIN 恢复密钥，知晓关闭后不再完整展示', 'I have saved this PIN recovery key safely')}
              </span>
            </label>

            <div className="flex items-center justify-between pt-2">
              <button
                type="button"
                onClick={() => {
                  const blob = new Blob([`Browsory PIN Recovery Key:\n${generatedRecoveryKey}\n\nGenerated at: ${new Date().toISOString()}`], { type: 'text/plain' });
                  const url = URL.createObjectURL(blob);
                  const a = document.createElement('a');
                  a.href = url;
                  a.download = `browsory-pin-recovery-key-${Date.now()}.txt`;
                  a.click();
                  URL.revokeObjectURL(url);
                  showToast(ui('已下载恢复密钥 TXT 文件', 'Downloaded recovery key TXT file'), 'success');
                }}
                className="px-3 py-1.5 rounded-xl border border-slate-300 dark:border-slate-700 hover:bg-slate-100 dark:hover:bg-slate-700 text-slate-700 dark:text-slate-200 text-xs flex items-center gap-1.5 transition cursor-pointer"
              >
                <FileText className="w-3.5 h-3.5 text-blue-500" />
                <span>{ui('保存为文本文件', 'Save as TXT')}</span>
              </button>

              <button
                type="button"
                disabled={!hasConfirmedPinRecoveryKey}
                onClick={() => {
                  setGeneratedRecoveryKey(null);
                  setHasConfirmedPinRecoveryKey(false);
                }}
                className="px-4 py-1.5 rounded-xl bg-blue-600 hover:bg-blue-700 disabled:opacity-40 text-white text-xs font-semibold shadow-md transition cursor-pointer"
              >
                {ui('我已妥善保存', 'Done')}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};
