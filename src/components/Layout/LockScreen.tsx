import React, { useState, useEffect } from 'react';
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
} from 'lucide-react';
import { useSecurityStore } from '../../stores/useSecurityStore';

export const LockScreen: React.FC = () => {
  const {
    unlock,
    isLockedOut,
    lockoutRemainingSecs,
    checkSecurityState,
    resetPinWithRecoveryKey,
  } = useSecurityStore();

  const [pin, setPin] = useState('');
  const [errorMsg, setErrorMsg] = useState('');
  const [isShaking, setIsShaking] = useState(false);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [countdown, setCountdown] = useState(lockoutRemainingSecs);

  // Recovery Key Modal State
  const [showRecoveryModal, setShowRecoveryModal] = useState(false);
  const [recoveryKeyInput, setRecoveryKeyInput] = useState('');
  const [newPinInput, setNewPinInput] = useState('');
  const [confirmPinInput, setConfirmPinInput] = useState('');
  const [recoveryError, setRecoveryError] = useState('');
  const [isRecovering, setIsRecovering] = useState(false);

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

  // Physical keyboard listener
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (showRecoveryModal || isLockedOut || countdown > 0 || isSubmitting) return;

      if (e.key >= '0' && e.key <= '9') {
        if (pin.length < 6) {
          handleDigitPress(e.key);
        }
      } else if (e.key === 'Backspace') {
        handleBackspace();
      } else if (e.key === 'Enter') {
        if (pin.length >= 4) {
          handleSubmit(pin);
        }
      } else if (e.key === 'Escape') {
        setPin('');
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [pin, showRecoveryModal, isLockedOut, countdown, isSubmitting]);

  const handleDigitPress = (digit: string) => {
    if (pin.length >= 6) return;
    setErrorMsg('');
    const newPin = pin + digit;
    setPin(newPin);

    // Auto submit if reached 6 digits or 4 digits check
    if (newPin.length === 6) {
      handleSubmit(newPin);
    }
  };

  const handleBackspace = () => {
    setErrorMsg('');
    setPin((prev) => prev.slice(0, -1));
  };

  const handleClear = () => {
    setErrorMsg('');
    setPin('');
  };

  const handleSubmit = async (pinToVerify = pin) => {
    if (pinToVerify.length < 4 || isSubmitting) return;
    setIsSubmitting(true);
    setErrorMsg('');

    try {
      const success = await unlock(pinToVerify);
      if (!success) {
        triggerShake('PIN 码错误');
      }
    } catch (err: any) {
      const msg = typeof err === 'string' ? err : err?.message || '解锁失败';
      triggerShake(msg);
    } finally {
      setIsSubmitting(false);
    }
  };

  const triggerShake = (msg: string) => {
    setIsShaking(true);
    setErrorMsg(msg);
    setPin('');
    setTimeout(() => setIsShaking(false), 500);
  };

  const handleRecoverySubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setRecoveryError('');

    const trimmedKey = recoveryKeyInput.trim();
    if (!trimmedKey) {
      setRecoveryError('请输入安全恢复密钥');
      return;
    }

    if (newPinInput.length < 4 || newPinInput.length > 6 || !/^\d+$/.test(newPinInput)) {
      setRecoveryError('新 PIN 码必须为 4 至 6 位纯数字');
      return;
    }

    if (newPinInput !== confirmPinInput) {
      setRecoveryError('两次输入的新 PIN 码不一致');
      return;
    }

    setIsRecovering(true);
    try {
      await resetPinWithRecoveryKey(trimmedKey, newPinInput);
      setShowRecoveryModal(false);
    } catch (err: any) {
      const msg = typeof err === 'string' ? err : err?.message || '恢复失败，恢复密钥无效';
      setRecoveryError(msg);
    } finally {
      setIsRecovering(false);
    }
  };

  const isBlocked = isLockedOut || countdown > 0;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/80 backdrop-blur-xl select-none animate-in fade-in duration-300">
      <div
        className={`w-full max-w-sm mx-4 p-8 rounded-3xl bg-white/95 dark:bg-slate-900/95 border border-slate-200/80 dark:border-slate-800/80 shadow-2xl flex flex-col items-center text-center transition-all ${
          isShaking ? 'animate-bounce' : ''
        }`}
      >
        {/* Shield Icon */}
        <div className="w-16 h-16 rounded-2xl bg-indigo-50 dark:bg-indigo-950/60 border border-indigo-200 dark:border-indigo-800/60 flex items-center justify-center text-indigo-600 dark:text-indigo-400 shadow-inner mb-4">
          <Shield className="w-8 h-8" />
        </div>

        <h2 className="text-xl font-bold tracking-tight text-slate-900 dark:text-slate-100">
          Browsory · 浏览足迹
        </h2>
        <p className="text-xs text-slate-500 dark:text-slate-400 mt-1 mb-6">
          应用界面已安全锁定，请输入 PIN 码解锁
        </p>

        {/* Lockout Banner */}
        {isBlocked ? (
          <div className="w-full p-4 rounded-xl bg-amber-50 dark:bg-amber-950/40 border border-amber-200 dark:border-amber-800/50 flex items-center gap-3 text-amber-700 dark:text-amber-300 mb-6">
            <Clock className="w-5 h-5 flex-shrink-0 animate-spin" />
            <div className="text-xs text-left">
              <div className="font-semibold">安全保护已启动</div>
              <div>请等待 {countdown} 秒后再次尝试</div>
            </div>
          </div>
        ) : (
          <>
            {/* PIN Dots Display */}
            <div className="flex items-center justify-center gap-3 mb-6 h-8">
              {[0, 1, 2, 3, 4, 5].map((index) => {
                const isFilled = index < pin.length;
                return (
                  <div
                    key={index}
                    className={`w-3.5 h-3.5 rounded-full transition-all duration-200 ${
                      isFilled
                        ? 'bg-indigo-600 dark:bg-indigo-400 scale-110 shadow-sm'
                        : 'bg-slate-200 dark:bg-slate-700'
                    }`}
                  />
                );
              })}
            </div>

            {/* Error Message */}
            {errorMsg && (
              <div className="flex items-center gap-1.5 text-xs text-rose-500 dark:text-rose-400 mb-4 animate-in fade-in">
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
              disabled={isBlocked || isSubmitting}
              onClick={() => handleDigitPress(digit)}
              className="h-14 rounded-2xl bg-slate-100 dark:bg-slate-800/70 hover:bg-slate-200 dark:hover:bg-slate-700/80 active:scale-95 transition text-lg font-medium text-slate-800 dark:text-slate-100 flex items-center justify-center disabled:opacity-40 disabled:pointer-events-none shadow-sm cursor-pointer"
            >
              {digit}
            </button>
          ))}
          <button
            disabled={isBlocked || isSubmitting || pin.length === 0}
            onClick={handleClear}
            className="h-14 rounded-2xl bg-slate-100/60 dark:bg-slate-800/40 hover:bg-slate-200/80 dark:hover:bg-slate-700/60 active:scale-95 transition text-xs font-medium text-slate-500 dark:text-slate-400 flex items-center justify-center disabled:opacity-30 disabled:pointer-events-none cursor-pointer"
          >
            清空
          </button>
          <button
            disabled={isBlocked || isSubmitting}
            onClick={() => handleDigitPress('0')}
            className="h-14 rounded-2xl bg-slate-100 dark:bg-slate-800/70 hover:bg-slate-200 dark:hover:bg-slate-700/80 active:scale-95 transition text-lg font-medium text-slate-800 dark:text-slate-100 flex items-center justify-center disabled:opacity-40 disabled:pointer-events-none shadow-sm cursor-pointer"
          >
            0
          </button>
          <button
            disabled={isBlocked || isSubmitting || pin.length === 0}
            onClick={handleBackspace}
            className="h-14 rounded-2xl bg-slate-100/60 dark:bg-slate-800/40 hover:bg-slate-200/80 dark:hover:bg-slate-700/60 active:scale-95 transition text-slate-600 dark:text-slate-300 flex items-center justify-center disabled:opacity-30 disabled:pointer-events-none cursor-pointer"
            title="删除"
          >
            <Delete className="w-5 h-5" />
          </button>
        </div>

        {/* Manual Submit Button if 4 or 5 digits entered */}
        {pin.length >= 4 && pin.length < 6 && !isBlocked && (
          <button
            onClick={() => handleSubmit()}
            disabled={isSubmitting}
            className="w-full max-w-[260px] py-2.5 px-4 rounded-xl bg-indigo-600 hover:bg-indigo-700 text-white font-medium text-sm flex items-center justify-center gap-2 shadow-lg shadow-indigo-600/20 active:scale-95 transition cursor-pointer"
          >
            <Lock className="w-4 h-4" />
            <span>确认解锁</span>
            <ArrowRight className="w-4 h-4" />
          </button>
        )}

        {/* Recovery Key Link */}
        <div className="mt-4 flex flex-col gap-1 items-center">
          <button
            type="button"
            onClick={() => setShowRecoveryModal(true)}
            className="text-[11px] text-indigo-600 dark:text-indigo-400 hover:underline flex items-center gap-1 cursor-pointer"
          >
            <KeyRound className="w-3 h-3" />
            <span>忘记 PIN 码？使用安全恢复密钥解锁</span>
          </button>
          <span className="text-[10px] text-slate-400 dark:text-slate-500">
            支持物理键盘直接按键 / 回车解锁
          </span>
        </div>
      </div>

      {/* Recovery Key Modal */}
      {showRecoveryModal && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/70 backdrop-blur-md p-4 animate-in fade-in">
          <div className="w-full max-w-md bg-white dark:bg-slate-900 rounded-3xl border border-slate-200 dark:border-slate-800 shadow-2xl p-6 flex flex-col gap-4">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2.5">
                <div className="w-9 h-9 rounded-xl bg-amber-50 dark:bg-amber-950/60 border border-amber-200 dark:border-amber-800/60 flex items-center justify-center text-amber-600 dark:text-amber-400">
                  <KeyRound className="w-4 h-4" />
                </div>
                <div>
                  <h3 className="font-bold text-sm text-slate-900 dark:text-slate-100">
                    使用安全恢复密钥重置 PIN
                  </h3>
                  <p className="text-[11px] text-slate-500">
                    验证紧急凭证并重新设置访问密码
                  </p>
                </div>
              </div>
              <button
                type="button"
                onClick={() => setShowRecoveryModal(false)}
                className="p-1 rounded-lg text-slate-400 hover:text-slate-600 dark:hover:text-slate-200 transition"
              >
                <X className="w-4 h-4" />
              </button>
            </div>

            <form onSubmit={handleRecoverySubmit} className="flex flex-col gap-3 text-xs">
              <div className="flex flex-col gap-1">
                <label className="text-slate-600 dark:text-slate-400 font-medium">
                  恢复密钥 (Recovery Key)
                </label>
                <input
                  type="text"
                  value={recoveryKeyInput}
                  onChange={(e) => setRecoveryKeyInput(e.target.value)}
                  placeholder="BHA-XXXX-XXXX-XXXX-..."
                  className="px-3 py-2 rounded-xl border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-950 font-mono text-xs focus:ring-2 focus:ring-indigo-500/20 focus:outline-none"
                  autoFocus
                />
              </div>

              <div className="flex flex-col gap-1">
                <label className="text-slate-600 dark:text-slate-400 font-medium">
                  设置新 PIN 码 (4~6 位数字)
                </label>
                <input
                  type="password"
                  maxLength={6}
                  value={newPinInput}
                  onChange={(e) => setNewPinInput(e.target.value)}
                  placeholder="请输入新 PIN 码"
                  className="px-3 py-2 rounded-xl border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-950 font-mono text-xs focus:ring-2 focus:ring-indigo-500/20 focus:outline-none"
                />
              </div>

              <div className="flex flex-col gap-1">
                <label className="text-slate-600 dark:text-slate-400 font-medium">
                  确认新 PIN 码
                </label>
                <input
                  type="password"
                  maxLength={6}
                  value={confirmPinInput}
                  onChange={(e) => setConfirmPinInput(e.target.value)}
                  placeholder="请再次输入新 PIN 码"
                  className="px-3 py-2 rounded-xl border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-950 font-mono text-xs focus:ring-2 focus:ring-indigo-500/20 focus:outline-none"
                />
              </div>

              {recoveryError && (
                <div className="p-2.5 rounded-xl bg-rose-50 dark:bg-rose-950/40 border border-rose-200 dark:border-rose-900/60 text-rose-600 dark:text-rose-400 flex items-center gap-1.5">
                  <AlertCircle className="w-3.5 h-3.5 shrink-0" />
                  <span>{recoveryError}</span>
                </div>
              )}

              <div className="flex items-center justify-end gap-2 pt-2">
                <button
                  type="button"
                  onClick={() => setShowRecoveryModal(false)}
                  disabled={isRecovering}
                  className="px-3 py-1.5 rounded-xl border border-slate-300 dark:border-slate-700 text-slate-600 dark:text-slate-400 hover:bg-slate-100 dark:hover:bg-slate-800"
                >
                  取消
                </button>
                <button
                  type="submit"
                  disabled={isRecovering}
                  className="px-4 py-1.5 rounded-xl bg-indigo-600 hover:bg-indigo-700 text-white font-medium flex items-center gap-1.5 shadow-md disabled:opacity-50"
                >
                  {isRecovering ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <CheckCircle2 className="w-3.5 h-3.5" />}
                  <span>{isRecovering ? '正在重置...' : '重置并解锁'}</span>
                </button>
              </div>
            </form>
          </div>
        </div>
      )}
    </div>
  );
};
