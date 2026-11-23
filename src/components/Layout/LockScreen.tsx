import React, { useState, useEffect, useRef } from 'react';
import {
  Shield,
  Lock,
  Delete,
  ArrowRight,
  AlertCircle,
  Clock,
  KeyRound,
  X,
  CheckCircle2,
  Loader2,
  Database,
  Copy,
  Check,
  FileText,
  Eye,
  EyeOff,
  AlertTriangle,
  RefreshCw,
} from 'lucide-react';
import { useSecurityStore } from '../../stores/useSecurityStore';
import { useAppStore } from '../../stores/useAppStore';
import { useDialogFocus } from '../../hooks/useDialogFocus';
import { generateVaultPassphrase } from '../../services/vaultPassphrase';

export const LockScreen: React.FC = () => {
  const { language } = useAppStore();
  const ui = (zh: string, en: string) => (language === 'en-US' ? en : zh);

  const {
    vaultStatus,
    hasDpapiWrap,
    passiveResumeError,
    pinRequiredOnResume,
    isUiLocked,
    unlock,
    unlockVault,
    unlockVaultWithRecoveryKey,
    createVault,
    migrateLegacyDatabase,
    lockVault,
    recoveryKeyPending,
    pendingRecoveryKey,
    confirmVaultRecoveryKey,
    regeneratePassiveVaultRecoveryKey,
    isLockedOut,
    lockoutRemainingSecs,
    checkSecurityState,
    resetPinWithRecoveryKey,
    vaultLockError,
    isVaultLocking,
    isVaultRestoring,
  } = useSecurityStore();

  const showPinLock =
    (vaultStatus === 'unlocked' && isUiLocked) ||
    (vaultStatus === 'locked' && hasDpapiWrap && pinRequiredOnResume);

  // Common UI State
  const [errorMsg, setErrorMsg] = useState('');
  const [isShaking, setIsShaking] = useState(false);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [showPassword, setShowPassword] = useState(false);

  // Vault Password & Setup State
  const [passwordInput, setPasswordInput] = useState('');
  const [confirmPasswordInput, setConfirmPasswordInput] = useState('');
  const [passphraseFeedback, setPassphraseFeedback] = useState('');
  const [recoveryKeyMode, setRecoveryKeyMode] = useState(Boolean(passiveResumeError && hasDpapiWrap));
  const [vaultRecoveryKeyInput, setVaultRecoveryKeyInput] = useState('');
  const [newlyGeneratedKey, setNewlyGeneratedKey] = useState<string | null>(null);
  const recoveryKeyToDisplay = vaultStatus === 'unlocked' ? newlyGeneratedKey ?? pendingRecoveryKey : null;
  const isVaultLockPending =
    isVaultLocking || vaultStatus === 'locking';
  const [hasConfirmedBackup, setHasConfirmedBackup] = useState(false);
  const [isCopied, setIsCopied] = useState(false);
  const [recoveryCopyError, setRecoveryCopyError] = useState('');

  useEffect(() => {
    if (passiveResumeError && vaultStatus === 'locked' && hasDpapiWrap) {
      setRecoveryKeyMode(true);
    }
  }, [passiveResumeError, vaultStatus, hasDpapiWrap]);

  // UI PIN Screen Lock State
  const [pin, setPin] = useState('');
  const [countdown, setCountdown] = useState(lockoutRemainingSecs);

  // UI PIN Recovery Modal State
  const [showPinRecoveryModal, setShowPinRecoveryModal] = useState(false);
  const [pinRecoveryKeyInput, setPinRecoveryKeyInput] = useState('');
  const [newPinInput, setNewPinInput] = useState('');
  const [confirmPinInput, setConfirmPinInput] = useState('');
  const [pinRecoveryError, setPinRecoveryError] = useState('');
  const [isRecoveringPin, setIsRecoveringPin] = useState(false);

  const pinRecoveryKeyRef = useRef<HTMLInputElement>(null);
  const pinRecoveryDialogRef = useDialogFocus<HTMLDivElement>({
    isOpen: showPinRecoveryModal,
    onClose: () => {
      if (!isRecoveringPin) setShowPinRecoveryModal(false);
    },
    initialFocusRef: pinRecoveryKeyRef,
  });

  useEffect(() => {
    if (vaultStatus !== 'unlocked') {
      setNewlyGeneratedKey(null);
      setHasConfirmedBackup(false);
    }
  }, [vaultStatus]);

  useEffect(() => {
    setCountdown(lockoutRemainingSecs);
  }, [lockoutRemainingSecs]);

  // Countdown timer when locked out
  useEffect(() => {
    if (countdown <= 0) return;
    const timer = setInterval(() => {
      setCountdown((prev) => {
        if (prev <= 1) {
          checkSecurityState();
          return 0;
        }
        return prev - 1;
      });
    }, 1000);
    return () => clearInterval(timer);
  }, [countdown]);

  // Physical keyboard listener for 4-digit PIN mode
  useEffect(() => {
    if (!showPinLock) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      if (showPinRecoveryModal || isLockedOut || countdown > 0 || isSubmitting) return;

      if (e.key >= '0' && e.key <= '9') {
        if (pin.length < 4) {
          handlePinDigitPress(e.key);
        }
      } else if (e.key === 'Backspace') {
        handlePinBackspace();
      } else if (e.key === 'Enter') {
        if (pin.length === 4) {
          handlePinSubmit(pin);
        }
      } else if (e.key === 'Escape') {
        setPin('');
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [pin, showPinLock, showPinRecoveryModal, isLockedOut, countdown, isSubmitting]);

  const triggerShake = (msg: string) => {
    setIsShaking(true);
    setErrorMsg(msg);
    setTimeout(() => setIsShaking(false), 500);
  };

  // --- PIN Handlers ---
  const handlePinDigitPress = (digit: string) => {
    if (pin.length >= 4) return;
    setErrorMsg('');
    const newPin = pin + digit;
    setPin(newPin);

    if (newPin.length === 4) {
      handlePinSubmit(newPin);
    }
  };

  const handlePinBackspace = () => {
    setErrorMsg('');
    setPin((prev) => prev.slice(0, -1));
  };

  const handlePinClear = () => {
    setErrorMsg('');
    setPin('');
  };

  const handlePinSubmit = async (pinToVerify = pin) => {
    if (pinToVerify.length < 4 || isSubmitting) return;
    setIsSubmitting(true);
    setErrorMsg('');

    try {
      const success = await unlock(pinToVerify);
      if (!success) {
        setPin('');
        triggerShake(ui('PIN 码错误', 'Incorrect PIN'));
      }
    } catch (err: any) {
      setPin('');
      const msg = typeof err === 'string' ? err : err?.message || ui('解锁失败', 'Unlock failed');
      triggerShake(msg);
    } finally {
      setIsSubmitting(false);
    }
  };

  const handlePinRecoverySubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setPinRecoveryError('');

    const trimmedKey = pinRecoveryKeyInput.trim();
    if (!trimmedKey) {
      setPinRecoveryError(ui('请输入安全恢复密钥', 'Enter your recovery key'));
      return;
    }

    if (!/^\d{4}$/.test(newPinInput)) {
      setPinRecoveryError(ui('新 PIN 码必须为 4 位纯数字', 'New PIN must be 4 digits'));
      return;
    }

    if (newPinInput !== confirmPinInput) {
      setPinRecoveryError(ui('两次输入的新 PIN 码不一致', 'The two PIN entries do not match'));
      return;
    }

    setIsRecoveringPin(true);
    try {
      await resetPinWithRecoveryKey(trimmedKey, newPinInput);
      setShowPinRecoveryModal(false);
      setPin('');
      setPinRecoveryKeyInput('');
      setNewPinInput('');
      setConfirmPinInput('');
    } catch (err: any) {
      const msg = typeof err === 'string' ? err : err?.message || ui('恢复失败，恢复密钥无效', 'Reset failed; invalid recovery key');
      setPinRecoveryError(msg);
    } finally {
      setIsRecoveringPin(false);
    }
  };

  // --- Vault Unlock Handlers ---
  const handleVaultUnlock = async (e: React.FormEvent) => {
    e.preventDefault();
    if (isSubmitting) return;

    if (recoveryKeyMode) {
      const trimmedKey = vaultRecoveryKeyInput.trim();
      if (!trimmedKey) {
        triggerShake(ui('请输入恢复密钥', 'Enter recovery key'));
        return;
      }
      setIsSubmitting(true);
      setErrorMsg('');
      try {
        const ok = await unlockVaultWithRecoveryKey(trimmedKey);
        if (!ok) {
          triggerShake(ui('恢复密钥无效，无法解封主密钥', 'Invalid recovery key; could not unwrap master key'));
        } else {
          setVaultRecoveryKeyInput('');
        }
      } catch (err: any) {
        triggerShake(typeof err === 'string' ? err : err?.message || ui('恢复密钥解锁失败', 'Recovery key unlock failed'));
      } finally {
        setIsSubmitting(false);
      }
    } else {
      if (!passwordInput) {
        triggerShake(ui('请输入数据库口令', 'Enter vault passphrase'));
        return;
      }
      setIsSubmitting(true);
      setErrorMsg('');
      try {
        const ok = await unlockVault(passwordInput);
        if (!ok) {
          triggerShake(ui('数据库加密口令错误', 'Incorrect vault passphrase'));
        } else {
          setPasswordInput('');
        }
      } catch (err: any) {
        triggerShake(typeof err === 'string' ? err : err?.message || ui('数据库解锁失败', 'Vault unlock failed'));
      } finally {
        setIsSubmitting(false);
      }
    }
  };

  // --- Vault Setup / Migration Handlers ---
  const handleGeneratePassphrase = () => {
    if (isSubmitting) return;
    try {
      const generated = generateVaultPassphrase();
      setPasswordInput(generated);
      setConfirmPasswordInput(generated);
      setShowPassword(true);
      setErrorMsg('');
      setPassphraseFeedback(ui('已生成并填入两次口令，请复制并妥善保存。', 'Generated and filled both fields. Copy and save your passphrase.'));
    } catch {
      setErrorMsg(ui('安全随机数不可用，未生成口令，请手动输入。', 'Secure randomness is unavailable. Enter a passphrase manually.'));
    }
  };

  const handleCopyPassphrase = async () => {
    if (isSubmitting || !passwordInput) return;
    try {
      await navigator.clipboard.writeText(passwordInput);
      setPassphraseFeedback(ui('口令已复制，请保存到密码管理器或其他安全位置。', 'Passphrase copied. Save it in your password manager or another safe place.'));
    } catch {
      setPassphraseFeedback(ui('复制失败，请显示口令后手动保存。', 'Copy failed. Show the passphrase and save it manually.'));
    }
  };

  const handleVaultSetup = async (e: React.FormEvent) => {
    e.preventDefault();
    if (isSubmitting) return;

    if (Array.from(passwordInput.trim()).length < 12) {
      triggerShake(ui(
        '数据库口令长度至少为 12 个字符，建议使用较长口令短语',
        'Passphrase must be at least 12 characters'
      ));
      return;
    }
    if (passwordInput !== confirmPasswordInput) {
      triggerShake(ui('两次输入的口令不一致', 'The two passphrases do not match'));
      return;
    }

    setIsSubmitting(true);
    setErrorMsg('');
    try {
      let recKey = '';
      if (vaultStatus === 'legacy_plaintext') {
        recKey = await migrateLegacyDatabase(passwordInput);
      } else {
        recKey = await createVault(passwordInput);
      }
      setNewlyGeneratedKey(recKey);
      setPasswordInput('');
      setConfirmPasswordInput('');
    } catch (err: any) {
      triggerShake(typeof err === 'string' ? err : err?.message || ui('保险库初始化失败', 'Vault setup failed'));
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleFinishSetupWithKey = async () => {
    if (!hasConfirmedBackup || !recoveryKeyToDisplay || isSubmitting) return;
    setIsSubmitting(true);
    try {
      await confirmVaultRecoveryKey(recoveryKeyToDisplay);
      setNewlyGeneratedKey(null);
      setHasConfirmedBackup(false);
    } catch (error: any) {
      triggerShake(typeof error === 'string' ? error : error?.message || ui('保存恢复密钥确认失败，请重试', 'Failed to confirm recovery key; please retry'));
    } finally {
      setIsSubmitting(false);
    }
  };

  const isBlocked = isLockedOut || countdown > 0;

  if (vaultLockError || isVaultLockPending || isVaultRestoring) {
    return (
      <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/80 backdrop-blur-xl select-none animate-in fade-in duration-300">
        <div className="w-full max-w-sm mx-4 p-8 rounded-3xl bg-white/95 dark:bg-slate-900/95 border border-slate-200/80 dark:border-slate-800/80 shadow-2xl flex flex-col items-center text-center">
          <div className={`w-16 h-16 rounded-2xl flex items-center justify-center shadow-inner mb-4 ${
            vaultLockError
              ? 'bg-rose-50 dark:bg-rose-950/60 border border-rose-200 dark:border-rose-800/60 text-rose-600 dark:text-rose-400'
              : 'bg-blue-50 dark:bg-blue-950/60 border border-blue-200 dark:border-blue-800/60 text-blue-600 dark:text-blue-400'
          }`}>
            {vaultLockError ? <AlertTriangle className="w-8 h-8" /> : <Loader2 className="w-8 h-8 animate-spin" />}
          </div>
          <h2 className="text-xl font-bold tracking-tight text-slate-900 dark:text-slate-100">
            {vaultLockError
              ? ui('保险库锁定未完成', 'Vault lock incomplete')
              : isVaultRestoring
              ? ui('正在校验并恢复备份', 'Verifying and restoring backup')
              : ui('正在锁定保险库', 'Locking vault')}
          </h2>
          <p className="text-xs text-slate-500 dark:text-slate-400 mt-1 mb-4 leading-relaxed">
            {vaultLockError
              ? ui(
                  '主界面已保护，数据库尚未确认关闭。请重试锁定；此时不能宣称数据已锁定。',
                  'The interface is hidden, but the database handle is still closing. Retry locking before treating the vault as closed.'
                )
              : isVaultRestoring
              ? ui(
                  '正在检查备份并准备切换，完成后需使用备份时的凭据解锁。',
                  'Validating backup and preparing switch. Unlock with the backup credentials when done.'
                )
              : ui('正在关闭数据库连接，请稍候。', 'Closing database connection, please wait.')}
          </p>
          {vaultLockError && (
            <div className="w-full p-3 mb-4 rounded-xl bg-rose-50/70 dark:bg-rose-950/40 border border-rose-200 dark:border-rose-900/60 text-xs text-rose-700 dark:text-rose-300 font-mono text-left break-all" role="alert">
              {vaultLockError}
            </div>
          )}
          {vaultLockError && (
            <div className="w-full flex flex-col gap-2">
              <button
                type="button"
                onClick={() => lockVault()}
                className="w-full py-2.5 px-4 rounded-xl bg-blue-600 hover:bg-blue-700 text-white text-xs font-semibold flex items-center justify-center gap-2 cursor-pointer shadow-md"
              >
                <RefreshCw className="w-4 h-4" />
                <span>{ui('重试锁定保险库', 'Retry locking vault')}</span>
              </button>
            </div>
          )}
        </div>
      </div>
    );
  }

  // 1. Newly Generated Recovery Key Card (Setup or Migration done)
  if (recoveryKeyToDisplay) {
    return (
      <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/80 backdrop-blur-xl select-none animate-in fade-in duration-300">
        <div className="w-full max-w-md mx-4 p-8 rounded-3xl bg-white/95 dark:bg-slate-900/95 border border-slate-200/80 dark:border-slate-800/80 shadow-2xl flex flex-col items-center text-center">
          <div className="w-16 h-16 rounded-2xl bg-amber-50 dark:bg-amber-950/60 border border-amber-200 dark:border-amber-800/60 flex items-center justify-center text-amber-600 dark:text-amber-400 shadow-inner mb-4">
            <KeyRound className="w-8 h-8" />
          </div>

          <h2 className="text-xl font-bold tracking-tight text-slate-900 dark:text-slate-100">
            {ui('已生成安全恢复密钥', 'Recovery key generated')}
          </h2>
          <p className="text-xs text-slate-500 dark:text-slate-400 mt-1 mb-4 leading-relaxed">
            {ui(
              '请保存本次显示的恢复密钥。若重新生成过密钥，之前的恢复密钥已失效。遗忘口令且丢失恢复密钥后将无法解密数据。',
              'Save this recovery key now. Previous keys are invalidated when rotated. Without your passphrase or this key, data cannot be decrypted.'
            )}
          </p>

          <div className="w-full p-4 bg-slate-100 dark:bg-slate-950 rounded-2xl border border-slate-200 dark:border-slate-800 font-mono text-xs text-slate-800 dark:text-slate-100 break-all select-all flex items-center justify-between gap-2 mb-4">
            <span className="font-semibold tracking-wider text-left">{recoveryKeyToDisplay}</span>
            <button
              type="button"
              onClick={async () => {
                setRecoveryCopyError('');
                try {
                  await navigator.clipboard.writeText(recoveryKeyToDisplay);
                  setIsCopied(true);
                  setTimeout(() => setIsCopied(false), 2000);
                } catch {
                  setRecoveryCopyError(ui('复制到剪贴板失败，请手动复制或导出文本文件。', 'Clipboard write failed; copy manually or download the text file.'));
                }
              }}
              aria-label={ui('复制到剪贴板', 'Copy to clipboard')}
              className="p-2 rounded-xl hover:bg-slate-200 dark:hover:bg-slate-800 text-slate-500 hover:text-slate-700 dark:hover:text-slate-200 transition shrink-0 cursor-pointer"
              title={ui('复制到剪贴板', 'Copy to clipboard')}
            >
              {isCopied ? <Check className="w-4 h-4 text-emerald-500" /> : <Copy className="w-4 h-4" />}
            </button>
          </div>

          {recoveryCopyError && (
            <p className="text-xs text-rose-500 mb-3 text-left w-full" role="alert">{recoveryCopyError}</p>
          )}

          <div className="flex gap-2 w-full mb-5">
            <button
              type="button"
              onClick={() => {
                const blob = new Blob(
                  [`Browsory Vault Recovery Key:\n${recoveryKeyToDisplay}\n\nGenerated: ${new Date().toISOString()}`],
                  { type: 'text/plain' }
                );
                const url = URL.createObjectURL(blob);
                const a = document.createElement('a');
                a.href = url;
                a.download = `browsory-vault-recovery-key-${Date.now()}.txt`;
                a.click();
                URL.revokeObjectURL(url);
              }}
              className="flex-1 py-2 px-3 rounded-xl border border-slate-200 dark:border-slate-800 hover:bg-slate-50 dark:hover:bg-slate-800/80 text-slate-700 dark:text-slate-300 text-xs font-medium flex items-center justify-center gap-1.5 transition cursor-pointer"
            >
              <FileText className="w-3.5 h-3.5 text-blue-500" />
              <span>{ui('保存为 TXT 文件', 'Save as TXT file')}</span>
            </button>
          </div>

          <label className="flex items-start gap-2.5 text-left text-xs text-slate-700 dark:text-slate-300 mb-6 cursor-pointer select-none">
            <input
              type="checkbox"
              checked={hasConfirmedBackup}
              onChange={(e) => setHasConfirmedBackup(e.target.checked)}
              className="mt-0.5 w-4 h-4 rounded text-blue-600 focus:ring-blue-500 border-slate-300 dark:border-slate-700"
            />
            <span className="leading-tight">
              {ui(
                '我已安全记录或妥善备份此恢复密钥，确认若丢失将自行承担无法恢复数据的后果。',
                'I have saved this recovery key securely and understand data cannot be recovered if lost.'
              )}
            </span>
          </label>

          {errorMsg && <p className="text-xs text-rose-500 mb-3" role="alert">{errorMsg}</p>}
          <button
            type="button"
            disabled={!hasConfirmedBackup || isSubmitting}
            onClick={handleFinishSetupWithKey}
            className="w-full py-3 px-4 rounded-xl bg-blue-600 hover:bg-blue-700 text-white font-semibold text-sm shadow-lg shadow-blue-600/20 active:scale-95 transition disabled:opacity-40 disabled:pointer-events-none flex items-center justify-center gap-2 cursor-pointer"
          >
            <span>{ui('进入 Browsory', 'Continue to Browsory')}</span>
            <ArrowRight className="w-4 h-4" />
          </button>
        </div>
      </div>
    );
  }

  // 2. Unconfigured or Legacy Plaintext Migration Mode
  if (vaultStatus === 'unconfigured' || vaultStatus === 'legacy_plaintext') {
    const isLegacy = vaultStatus === 'legacy_plaintext';
    return (
      <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/80 backdrop-blur-xl select-none animate-in fade-in duration-300">
        <div
          className={`w-full max-w-md mx-4 p-8 rounded-3xl bg-white/95 dark:bg-slate-900/95 border border-slate-200/80 dark:border-slate-800/80 shadow-2xl flex flex-col items-center text-center transition-all ${
            isShaking ? 'animate-bounce' : ''
          }`}
        >
          <div className="w-16 h-16 rounded-2xl bg-blue-50 dark:bg-blue-950/60 border border-blue-200 dark:border-blue-800/60 flex items-center justify-center text-blue-600 dark:text-blue-400 shadow-inner mb-4">
            {isLegacy ? <Database className="w-8 h-8" /> : <Shield className="w-8 h-8" />}
          </div>

          <h2 className="text-xl font-bold tracking-tight text-slate-900 dark:text-slate-100">
            {isLegacy
              ? ui('升级数据库安全保险库', 'Upgrade encrypted database vault')
              : ui('创建数据安全保险库', 'Create encrypted database vault')}
          </h2>
          <p className="text-xs text-slate-500 dark:text-slate-400 mt-1 mb-6 leading-relaxed">
            {isLegacy
              ? ui(
                  '检测到历史数据库。Browsory 将安全地将您的全部历史足迹、书签与标签迁移至 AES-256 静态加密保险库。请设置独立的数据库加密口令。',
                  'Existing archive detected. Browsory will migrate your history, bookmarks, and tags into an AES-256 SQLCipher vault protected by your passphrase.'
                )
              : ui(
                  'Browsory 采用 SQLCipher v4 (AES-256-CBC) 静态加密配合 Argon2id 密钥信封。请设置独立的数据库加密口令。',
                  'Browsory encrypts your local archive with SQLCipher v4 (AES-256-CBC) and an Argon2id key envelope. Choose a strong vault passphrase.'
                )}
          </p>

          <form onSubmit={handleVaultSetup} className="w-full flex flex-col gap-3.5 text-xs text-left">
            <div className="flex flex-col gap-1.5">
              <label className="text-slate-700 dark:text-slate-300 font-medium">
                {ui('数据库加密口令 (至少 12 个字符)', 'Vault passphrase (at least 12 characters)')}
              </label>
              <div className="relative">
                <input
                  type={showPassword ? 'text' : 'password'}
                  value={passwordInput}
                  onChange={(e) => { setPasswordInput(e.target.value); setPassphraseFeedback(''); }}
                  placeholder={ui('输入高强度口令', 'Enter strong passphrase')}
                  className="w-full px-3.5 py-2.5 rounded-xl border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-950 text-slate-900 dark:text-slate-100 font-medium text-xs focus:ring-2 focus:ring-blue-500/20 focus:outline-none pr-10"
                  autoFocus
                />
                <button
                  type="button"
                  onClick={() => setShowPassword((prev) => !prev)}
                  aria-label={showPassword ? ui('隐藏口令', 'Hide passphrase') : ui('显示口令', 'Show passphrase')}
                  className="absolute right-3 top-2.5 text-slate-400 hover:text-slate-600 dark:hover:text-slate-300 cursor-pointer"
                >
                  {showPassword ? <EyeOff className="w-4 h-4" /> : <Eye className="w-4 h-4" />}
                </button>
              </div>
              <div className="flex items-center gap-3 pt-1">
                <button type="button" onClick={handleGeneratePassphrase} disabled={isSubmitting}
                  className="inline-flex items-center gap-1.5 text-blue-600 dark:text-blue-400 hover:underline disabled:opacity-40">
                  <RefreshCw className="w-3.5 h-3.5" />
                  {ui('随机生成口令', 'Generate passphrase')}
                </button>
                <button type="button" onClick={handleCopyPassphrase} disabled={isSubmitting || !passwordInput}
                  className="inline-flex items-center gap-1.5 text-slate-600 dark:text-slate-300 hover:underline disabled:opacity-40">
                  <Copy className="w-3.5 h-3.5" />
                  {ui('复制口令', 'Copy passphrase')}
                </button>
              </div>
              <p role="status" className="text-[11px] leading-relaxed text-slate-500 dark:text-slate-400">
                {passphraseFeedback || ui('可一键生成 20 位随机口令，并自动填入确认框。', 'Generate a 20-character random passphrase and fill both fields automatically.')}
              </p>
            </div>

            <div className="flex flex-col gap-1.5">
              <label className="text-slate-700 dark:text-slate-300 font-medium">{ui('确认加密口令', 'Confirm passphrase')}</label>
              <input
                type={showPassword ? 'text' : 'password'}
                value={confirmPasswordInput}
                onChange={(e) => setConfirmPasswordInput(e.target.value)}
                placeholder={ui('再次输入口令', 'Re-enter passphrase')}
                className="w-full px-3.5 py-2.5 rounded-xl border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-950 text-slate-900 dark:text-slate-100 font-medium text-xs focus:ring-2 focus:ring-blue-500/20 focus:outline-none"
              />
            </div>

            {errorMsg && (
              <div role="alert" className="flex items-center gap-1.5 text-xs text-rose-500 dark:text-rose-400 pt-1">
                <AlertCircle className="w-4 h-4 shrink-0" />
                <span>{errorMsg}</span>
              </div>
            )}

            <button
              type="submit"
              disabled={isSubmitting || !passwordInput || !confirmPasswordInput}
              className="mt-3 w-full py-3 px-4 rounded-xl bg-blue-600 hover:bg-blue-700 text-white font-semibold text-sm shadow-lg shadow-blue-600/20 active:scale-95 transition flex items-center justify-center gap-2 disabled:opacity-40 disabled:pointer-events-none cursor-pointer"
            >
              {isSubmitting ? (
                <>
                  <Loader2 className="w-4 h-4 animate-spin" />
                  <span>{isLegacy ? ui('正在安全加密迁移...', 'Encrypting archive...') : ui('正在初始化保险库...', 'Initializing vault...')}</span>
                </>
              ) : (
                <>
                  <Lock className="w-4 h-4" />
                  <span>{isLegacy ? ui('开始安全加密迁移', 'Migrate and encrypt archive') : ui('创建并加密保险库', 'Create encrypted vault')}</span>
                  <ArrowRight className="w-4 h-4" />
                </>
              )}
            </button>
          </form>
        </div>
      </div>
    );
  }

  // 3. Vault Locked Mode (Database Connection is Closed)
  // Passive DPAPI resume prompt (only when locked without PIN, e.g. immediately after sleep/wake)
  if (vaultStatus === 'locked' && hasDpapiWrap && !pinRequiredOnResume && !passiveResumeError) {
    return (
      <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/80 backdrop-blur-xl select-none animate-in fade-in duration-300">
        <div className="w-full max-w-sm mx-4 p-8 rounded-3xl bg-white/95 dark:bg-slate-900/95 border border-slate-200/80 dark:border-slate-800/80 shadow-2xl flex flex-col items-center text-center">
          <div className="w-16 h-16 rounded-2xl bg-blue-50 dark:bg-blue-950/60 border border-blue-200 dark:border-blue-800/60 flex items-center justify-center text-blue-600 dark:text-blue-400 shadow-inner mb-4">
            <Shield className="w-8 h-8" />
          </div>
          <h2 className="text-xl font-bold tracking-tight text-slate-900 dark:text-slate-100">
            {ui('本地加密工作区已休眠', 'Encrypted workspace paused')}
          </h2>
          <p className="text-xs text-slate-500 dark:text-slate-400 mt-1 mb-6 leading-relaxed">
            {ui(
              '后台数据库已使用本机系统密钥安全加密。点击下方按钮即可快速恢复访问。',
              'Your archive is encrypted at rest via local OS key protection. Click below to resume access.'
            )}
          </p>
          <button
            type="button"
            disabled={isSubmitting}
            onClick={async () => {
              setIsSubmitting(true);
              try {
                await checkSecurityState();
              } finally {
                setIsSubmitting(false);
              }
            }}
            className="w-full py-2.5 px-4 rounded-xl bg-blue-600 hover:bg-blue-700 text-white text-xs font-semibold flex items-center justify-center gap-2 cursor-pointer shadow-md disabled:opacity-50"
          >
            {isSubmitting ? <Loader2 className="w-4 h-4 animate-spin" /> : <RefreshCw className="w-4 h-4" />}
            <span>{isSubmitting ? ui('正在安全唤醒...', 'Resuming...') : ui('恢复访问', 'Resume workspace')}</span>
          </button>
        </div>
      </div>
    );
  }

  // 3. Passive DPAPI 4-digit PIN Lock Mode or UI PIN Lock Mode
  // Must run before the legacy passphrase locked branch when a DPAPI vault
  // pauses on startup/lock waiting for 4-digit PIN verification.
  if (showPinLock) {
    return (
      <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/80 backdrop-blur-xl select-none animate-in fade-in duration-300">
        <div
          className={`w-full max-w-sm mx-4 p-8 rounded-3xl bg-white/95 dark:bg-slate-900/95 border border-slate-200/80 dark:border-slate-800/80 shadow-2xl flex flex-col items-center text-center transition-all ${
            isShaking ? 'animate-bounce' : ''
          }`}
        >
          <div className="w-16 h-16 rounded-2xl bg-blue-50 dark:bg-blue-950/60 border border-blue-200 dark:border-blue-800/60 flex items-center justify-center text-blue-600 dark:text-blue-400 shadow-inner mb-4">
            <Shield className="w-8 h-8" />
          </div>

          <h2 className="text-xl font-bold tracking-tight text-slate-900 dark:text-slate-100">
            {ui('应用已锁定', 'Screen locked')}
          </h2>
          <p className="text-xs text-slate-500 dark:text-slate-400 mt-1 mb-6">
            {ui('请输入 4 位 PIN 码以继续访问', 'Enter your 4-digit PIN to continue')}
          </p>

          {isBlocked ? (
            <div className="w-full p-4 rounded-xl bg-amber-50 dark:bg-amber-950/40 border border-amber-200 dark:border-amber-800/50 flex items-center gap-3 text-amber-700 dark:text-amber-300 mb-6">
              <Clock className="w-5 h-5 flex-shrink-0 animate-spin" />
              <div className="text-xs text-left">
                <div className="font-semibold">{ui('安全保护已启动', 'Cooldown active')}</div>
                <div>{ui(`请等待 ${countdown} 秒后再次尝试`, `Please wait ${countdown}s before retrying`)}</div>
              </div>
            </div>
          ) : (
            <>
              {/* 4-digit PIN Dots */}
              <div className="flex items-center justify-center gap-4 mb-6 h-8">
                {[0, 1, 2, 3].map((index) => {
                  const isFilled = index < pin.length;
                  return (
                    <div
                      key={index}
                      className={`w-3.5 h-3.5 rounded-full transition-all duration-200 ${
                        isFilled
                          ? 'bg-blue-600 dark:bg-blue-400 scale-110 shadow-sm'
                          : 'bg-slate-200 dark:bg-slate-700'
                      }`}
                    />
                  );
                })}
              </div>

              {errorMsg && (
                <div role="alert" className="flex items-center gap-1.5 text-xs text-rose-500 dark:text-rose-400 mb-4 animate-in fade-in">
                  <AlertCircle className="w-4 h-4" />
                  <span>{errorMsg}</span>
                </div>
              )}
            </>
          )}

          {/* Numeric Keypad */}
          <div className="grid grid-cols-3 gap-3 w-full max-w-[260px] mb-4">
            {['1', '2', '3', '4', '5', '6', '7', '8', '9'].map((digit) => (
              <button
                key={digit}
                type="button"
                disabled={isBlocked || isSubmitting}
                onClick={() => handlePinDigitPress(digit)}
                className="h-14 rounded-2xl bg-slate-100 dark:bg-slate-800/70 hover:bg-slate-200 dark:hover:bg-slate-700/80 active:scale-95 transition text-lg font-medium text-slate-800 dark:text-slate-100 flex items-center justify-center disabled:opacity-40 disabled:pointer-events-none shadow-sm cursor-pointer"
              >
                {digit}
              </button>
            ))}
            <button
              type="button"
              disabled={isBlocked || isSubmitting || pin.length === 0}
              onClick={handlePinClear}
              className="h-14 rounded-2xl bg-slate-100/60 dark:bg-slate-800/40 hover:bg-slate-200/80 dark:hover:bg-slate-700/60 active:scale-95 transition text-xs font-medium text-slate-500 dark:text-slate-400 flex items-center justify-center disabled:opacity-30 disabled:pointer-events-none cursor-pointer"
            >
              {ui('清空', 'Clear')}
            </button>
            <button
              type="button"
              disabled={isBlocked || isSubmitting}
              onClick={() => handlePinDigitPress('0')}
              className="h-14 rounded-2xl bg-slate-100 dark:bg-slate-800/70 hover:bg-slate-200 dark:hover:bg-slate-700/80 active:scale-95 transition text-lg font-medium text-slate-800 dark:text-slate-100 flex items-center justify-center disabled:opacity-40 disabled:pointer-events-none shadow-sm cursor-pointer"
            >
              0
            </button>
            <button
              type="button"
              disabled={isBlocked || isSubmitting || pin.length === 0}
              onClick={handlePinBackspace}
              aria-label={ui('退格删除', 'Backspace')}
              className="h-14 rounded-2xl bg-slate-100/60 dark:bg-slate-800/40 hover:bg-slate-200/80 dark:hover:bg-slate-700/60 active:scale-95 transition text-slate-600 dark:text-slate-300 flex items-center justify-center disabled:opacity-30 disabled:pointer-events-none cursor-pointer"
              title={ui('删除', 'Backspace')}
            >
              <Delete className="w-5 h-5" />
            </button>
          </div>

          {/* Quick Vault Lock & PIN Recovery Actions */}
          {(vaultStatus === 'unlocked' || (vaultStatus === 'locked' && hasDpapiWrap && pinRequiredOnResume)) && (
            <div className="w-full pt-2 border-t border-slate-100 dark:border-slate-800/80 flex flex-col gap-2 items-center">
              {vaultStatus === 'unlocked' && !hasDpapiWrap && (
                <button
                  type="button"
                  onClick={async () => {
                    await lockVault();
                  }}
                  className="w-full py-2 px-3 rounded-xl border border-slate-200 dark:border-slate-700/80 hover:bg-slate-100 dark:hover:bg-slate-800 text-slate-600 dark:text-slate-300 text-xs font-medium flex items-center justify-center gap-1.5 transition cursor-pointer"
                >
                  <Lock className="w-3.5 h-3.5 text-rose-500" />
                  <span>{ui('彻底锁定并关闭数据库保险库', 'Close and lock database vault')}</span>
                </button>
              )}

              <button
                type="button"
                onClick={() => setShowPinRecoveryModal(true)}
                className="text-xs text-blue-600 dark:text-blue-400 hover:underline flex items-center gap-1 cursor-pointer"
              >
                <KeyRound className="w-3 h-3" />
                <span>{ui('忘记 PIN 码？使用安全恢复密钥重置', 'Forgot PIN? Reset with recovery key')}</span>
              </button>
            </div>
          )}
        </div>

        {/* UI PIN Recovery Modal */}
        {showPinRecoveryModal && (
          <div
            className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/70 backdrop-blur-md p-4 animate-in fade-in"
            onClick={() => {
              if (!isRecoveringPin) setShowPinRecoveryModal(false);
            }}
            role="presentation"
          >
            <div
              ref={pinRecoveryDialogRef}
              role="dialog"
              aria-modal="true"
              aria-labelledby="pin-recovery-modal-title"
              tabIndex={-1}
              onClick={(e) => e.stopPropagation()}
              className="w-full max-w-md bg-white dark:bg-slate-900 rounded-3xl border border-slate-200 dark:border-slate-800 shadow-2xl p-6 flex flex-col gap-4"
            >
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2.5">
                  <div className="w-9 h-9 rounded-xl bg-amber-50 dark:bg-amber-950/60 border border-amber-200 dark:border-amber-800/60 flex items-center justify-center text-amber-600 dark:text-amber-400">
                    <KeyRound className="w-4 h-4" />
                  </div>
                  <div>
                    <h3 id="pin-recovery-modal-title" className="font-bold text-sm text-slate-900 dark:text-slate-100">
                      {ui('使用安全恢复密钥重置 PIN', 'Reset PIN with recovery key')}
                    </h3>
                    <p className="text-xs text-slate-500">
                      {ui('验证紧急凭据并重新设置访问 PIN', 'Verify recovery key and set a new PIN')}
                    </p>
                  </div>
                </div>
                <button
                  type="button"
                  onClick={() => {
                    if (!isRecoveringPin) setShowPinRecoveryModal(false);
                  }}
                  disabled={isRecoveringPin}
                  aria-label={ui('关闭 PIN 重置窗口', 'Close PIN reset dialog')}
                  className="p-1 rounded-lg text-slate-400 hover:text-slate-600 dark:hover:text-slate-200 transition cursor-pointer disabled:opacity-40"
                >
                  <X className="w-4 h-4" />
                </button>
              </div>

              <form onSubmit={handlePinRecoverySubmit} className="flex flex-col gap-3 text-xs">
                <div className="flex flex-col gap-1">
                  <label className="text-slate-600 dark:text-slate-400 font-medium">
                    {ui('恢复密钥 (Recovery Key)', 'Recovery Key')}
                  </label>
                  <input
                    ref={pinRecoveryKeyRef}
                    type="text"
                    value={pinRecoveryKeyInput}
                    onChange={(e) => setPinRecoveryKeyInput(e.target.value)}
                    placeholder="BHA-XXXX-XXXX-XXXX-..."
                    className="px-3 py-2 rounded-xl border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-950 font-mono text-xs focus:ring-2 focus:ring-blue-500/20 focus:outline-none"
                  />
                </div>

                <div className="flex flex-col gap-1">
                  <label className="text-slate-600 dark:text-slate-400 font-medium">
                    {ui('设置新 PIN 码 (4 位数字)', 'New PIN (4 digits)')}
                  </label>
                  <input
                    type="password"
                    maxLength={4}
                    inputMode="numeric"
                    value={newPinInput}
                    onChange={(e) => setNewPinInput(e.target.value.replace(/\D/g, '').slice(0, 4))}
                    placeholder={ui('请输入 4 位数字 PIN 码', 'Enter 4-digit PIN')}
                    className="px-3 py-2 rounded-xl border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-950 font-mono text-xs focus:ring-2 focus:ring-blue-500/20 focus:outline-none"
                  />
                </div>

                <div className="flex flex-col gap-1">
                  <label className="text-slate-600 dark:text-slate-400 font-medium">
                    {ui('确认新 PIN 码', 'Confirm new PIN')}
                  </label>
                  <input
                    type="password"
                    maxLength={4}
                    inputMode="numeric"
                    value={confirmPinInput}
                    onChange={(e) => setConfirmPinInput(e.target.value.replace(/\D/g, '').slice(0, 4))}
                    placeholder={ui('请再次输入 4 位新 PIN 码', 'Re-enter 4-digit PIN')}
                    className="px-3 py-2 rounded-xl border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-950 font-mono text-xs focus:ring-2 focus:ring-blue-500/20 focus:outline-none"
                  />
                </div>

                {pinRecoveryError && (
                  <div role="alert" className="p-2.5 rounded-xl bg-rose-50 dark:bg-rose-950/40 border border-rose-200 dark:border-rose-900/60 text-rose-600 dark:text-rose-400 flex items-center gap-1.5">
                    <AlertCircle className="w-3.5 h-3.5 shrink-0" />
                    <span>{pinRecoveryError}</span>
                  </div>
                )}

                <div className="flex items-center justify-end gap-2 pt-2">
                  <button
                    type="button"
                    onClick={() => setShowPinRecoveryModal(false)}
                    disabled={isRecoveringPin}
                    className="px-3 py-1.5 rounded-xl border border-slate-300 dark:border-slate-700 text-slate-600 dark:text-slate-400 hover:bg-slate-100 dark:hover:bg-slate-800 cursor-pointer"
                  >
                    {ui('取消', 'Cancel')}
                  </button>
                  <button
                    type="submit"
                    disabled={isRecoveringPin}
                    className="px-4 py-1.5 rounded-xl bg-blue-600 hover:bg-blue-700 text-white font-medium flex items-center gap-1.5 shadow-md disabled:opacity-50 cursor-pointer"
                  >
                    {isRecoveringPin ? (
                      <Loader2 className="w-3.5 h-3.5 animate-spin" />
                    ) : (
                      <CheckCircle2 className="w-3.5 h-3.5" />
                    )}
                    <span>{isRecoveringPin ? ui('正在重置...', 'Resetting...') : ui('重置并解锁', 'Reset and unlock')}</span>
                  </button>
                </div>
              </form>
            </div>
          </div>
        )}
      </div>
    );
  }

  // 4. Legacy Passphrase Locked Mode (when !hasDpapiWrap) or Recovery Key Pending Modal
  if (vaultStatus === 'locked' || vaultStatus === 'unlocking' || (vaultStatus === 'unlocked' && recoveryKeyPending)) {
    return (
      <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/80 backdrop-blur-xl select-none animate-in fade-in duration-300">
        <div
          className={`w-full max-w-sm mx-4 p-8 rounded-3xl bg-white/95 dark:bg-slate-900/95 border border-slate-200/80 dark:border-slate-800/80 shadow-2xl flex flex-col items-center text-center transition-all ${
            isShaking ? 'animate-bounce' : ''
          }`}
        >
          <div className="w-16 h-16 rounded-2xl bg-blue-50 dark:bg-blue-950/60 border border-blue-200 dark:border-blue-800/60 flex items-center justify-center text-blue-600 dark:text-blue-400 shadow-inner mb-4">
            <Lock className="w-8 h-8" />
          </div>

          <h2 className="text-xl font-bold tracking-tight text-slate-900 dark:text-slate-100">
            {vaultStatus === 'unlocked' && recoveryKeyPending
              ? ui('请完成恢复密钥备份', 'Confirm recovery key backup')
              : ui('数据库保险库已锁定', 'Database vault locked')}
          </h2>
          <p className="text-xs text-slate-500 dark:text-slate-400 mt-1 mb-6">
            {passiveResumeError && vaultStatus === 'locked'
              ? ui('本机密钥唤醒失败。请使用已保存的恢复密钥解锁；如果此保险库使用口令，也可切换到口令解锁。', 'Local key resume failed. Unlock with your saved recovery key, or switch to the passphrase if this vault uses one.')
              : recoveryKeyMode
              ? ui('请输入安全恢复密钥 (BHA-XXXX-...) 以解封主密钥', 'Enter your recovery key (BHA-XXXX-...) to unwrap the vault key')
              : recoveryKeyPending && vaultStatus === 'unlocked'
                ? (hasDpapiWrap
                  ? ui('请生成并确认恢复密钥。迁移副本会继续保留；确认后可在维护设置中主动移除。', 'Generate and confirm a recovery key. The migration copy remains until you explicitly retire it in Maintenance settings.')
                  : ui('请输入口令重新生成恢复密钥，再确认保存；之前的恢复密钥将失效', 'Enter your passphrase to reissue and confirm your recovery key'))
                : ui('请输入数据库主加密口令以解密并装载历史档案', 'Enter your vault passphrase to decrypt and open your archive')}
          </p>

          {passiveResumeError && vaultStatus === 'locked' && (
            <div role="alert" className="w-full mb-3 text-left text-xs text-rose-500 dark:text-rose-400">{passiveResumeError}</div>
          )}

          <form onSubmit={handleVaultUnlock} className="w-full flex flex-col gap-3 text-xs text-left">
            {recoveryKeyMode ? (
              <div className="flex flex-col gap-1.5">
                <label className="text-slate-700 dark:text-slate-300 font-medium">{ui('恢复密钥', 'Recovery key')}</label>
                <input
                  type="text"
                  value={vaultRecoveryKeyInput}
                  onChange={(e) => setVaultRecoveryKeyInput(e.target.value)}
                  placeholder="BHA-XXXX-XXXX-XXXX-..."
                  className="w-full px-3.5 py-2.5 rounded-xl border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-950 text-slate-900 dark:text-slate-100 font-mono text-xs focus:ring-2 focus:ring-blue-500/20 focus:outline-none"
                  autoFocus
                />
              </div>
            ) : recoveryKeyPending && vaultStatus === 'unlocked' && hasDpapiWrap && !pendingRecoveryKey ? (
              <div className="text-xs text-slate-500 dark:text-slate-400">
                {passiveResumeError && <div role="alert" className="mb-2 text-rose-500 dark:text-rose-400">{passiveResumeError}</div>}
                <button
                  type="button"
                  disabled={isSubmitting}
                  onClick={async () => {
                    setIsSubmitting(true);
                    setErrorMsg('');
                    try {
                      await regeneratePassiveVaultRecoveryKey();
                    } catch (error) {
                      setErrorMsg(error instanceof Error ? error.message : String(error));
                    } finally {
                      setIsSubmitting(false);
                    }
                  }}
                  className="w-full py-2.5 px-4 rounded-xl bg-blue-600 hover:bg-blue-700 text-white text-xs font-semibold disabled:opacity-50"
                >{ui('重新生成恢复密钥', 'Generate recovery key')}</button>
              </div>
            ) : (
              <div className="flex flex-col gap-1.5">
                <label className="text-slate-700 dark:text-slate-300 font-medium">{ui('主加密口令', 'Vault passphrase')}</label>
                <div className="relative">
                  <input
                    type={showPassword ? 'text' : 'password'}
                    value={passwordInput}
                    onChange={(e) => setPasswordInput(e.target.value)}
                    placeholder={ui('输入主加密口令', 'Enter vault passphrase')}
                    className="w-full px-3.5 py-2.5 rounded-xl border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-950 text-slate-900 dark:text-slate-100 font-medium text-xs focus:ring-2 focus:ring-blue-500/20 focus:outline-none pr-10"
                    autoFocus
                  />
                  <button
                    type="button"
                    onClick={() => setShowPassword((prev) => !prev)}
                    aria-label={showPassword ? ui('隐藏口令', 'Hide passphrase') : ui('显示口令', 'Show passphrase')}
                    className="absolute right-3 top-2.5 text-slate-400 hover:text-slate-600 dark:hover:text-slate-300 cursor-pointer"
                  >
                    {showPassword ? <EyeOff className="w-4 h-4" /> : <Eye className="w-4 h-4" />}
                  </button>
                </div>
              </div>
            )}

            {errorMsg && (
              <div role="alert" className="flex items-center gap-1.5 text-xs text-rose-500 dark:text-rose-400">
                <AlertCircle className="w-4 h-4 shrink-0" />
                <span>{errorMsg}</span>
              </div>
            )}

            <button
              type="submit"
              disabled={isSubmitting || (recoveryKeyPending && vaultStatus === 'unlocked' && hasDpapiWrap && !pendingRecoveryKey) || (recoveryKeyMode ? !vaultRecoveryKeyInput : !passwordInput)}
              className="mt-2 w-full py-3 px-4 rounded-xl bg-blue-600 hover:bg-blue-700 text-white font-semibold text-sm shadow-lg shadow-blue-600/20 active:scale-95 transition flex items-center justify-center gap-2 disabled:opacity-40 disabled:pointer-events-none cursor-pointer"
            >
              {isSubmitting ? (
                <>
                  <Loader2 className="w-4 h-4 animate-spin" />
                  <span>{ui('正在派生密钥并解密数据库...', 'Deriving key and unlocking database...')}</span>
                </>
              ) : (
                <>
                  <Lock className="w-4 h-4" />
                  <span>{ui('解锁保险库', 'Unlock vault')}</span>
                  <ArrowRight className="w-4 h-4" />
                </>
              )}
            </button>

            <div className="text-center mt-3">
              <button
                type="button"
                onClick={() => {
                  setErrorMsg('');
                  setRecoveryKeyMode((prev) => !prev);
                }}
                className="text-xs text-blue-600 dark:text-blue-400 hover:underline cursor-pointer"
              >
                {recoveryKeyMode
                  ? ui('返回口令解锁', 'Unlock with passphrase instead')
                  : ui('忘记口令？使用安全恢复密钥解锁', 'Forgot passphrase? Unlock with recovery key')}
              </button>
            </div>
          </form>
        </div>
      </div>
    );
  }

  // 5. Error State fallback
  const errorDetails =
    typeof vaultStatus === 'object' && vaultStatus !== null && 'error' in vaultStatus
      ? (vaultStatus as { error: string }).error
      : null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/80 backdrop-blur-xl select-none animate-in fade-in duration-300">
      <div className="w-full max-w-sm mx-4 p-8 rounded-3xl bg-white/95 dark:bg-slate-900/95 border border-slate-200/80 dark:border-slate-800/80 shadow-2xl flex flex-col items-center text-center">
        <div className="w-16 h-16 rounded-2xl bg-rose-50 dark:bg-rose-950/60 border border-rose-200 dark:border-rose-800/60 flex items-center justify-center text-rose-600 dark:text-rose-400 shadow-inner mb-4">
          <AlertTriangle className="w-8 h-8" />
        </div>
        <h2 className="text-xl font-bold tracking-tight text-slate-900 dark:text-slate-100">
          {ui('保险库异常状态', 'Vault error')}
        </h2>
        <p className="text-xs text-slate-500 dark:text-slate-400 mt-1 mb-4 leading-relaxed">
          {errorDetails || ui(
            '当前保险库状态处于异常或不可用状态，请检查本地环境或从备份恢复。',
            'The vault encountered an error. Check your local environment or restore from an encrypted backup.'
          )}
        </p>
        {errorDetails && (
          <div role="alert" className="w-full p-3 mb-4 rounded-xl bg-rose-50/70 dark:bg-rose-950/40 border border-rose-200 dark:border-rose-900/60 text-xs text-rose-700 dark:text-rose-300 font-mono text-left break-all">
            {errorDetails}
          </div>
        )}
        <button
          type="button"
          onClick={() => checkSecurityState()}
          className="w-full py-2.5 px-4 rounded-xl bg-blue-600 hover:bg-blue-700 text-white text-xs font-semibold flex items-center justify-center gap-2 cursor-pointer shadow-md"
        >
          <RefreshCw className="w-4 h-4" />
          <span>{ui('重新检测与自愈状态', 'Re-check vault status')}</span>
        </button>
      </div>
    </div>
  );
};
