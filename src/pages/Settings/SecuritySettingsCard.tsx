import React, { useState, useEffect } from 'react';
import {
  Shield,
  Lock,
  KeyRound,
  Plus,
  Trash2,
  AlertCircle,
  X,
  Copy,
  Check,
  FileText,
} from 'lucide-react';
import { useSecurityStore } from '../../stores/useSecurityStore';
import { useAppStore } from '../../stores/useAppStore';
import { tauriApi } from '../../services/tauri';
import { PrivacyRule } from '../../types';

export const SecuritySettingsCard: React.FC = () => {
  const { showToast } = useAppStore();
  const {
    pinEnabled,
    autoLockMinutes,
    lockOnMinimize,
    lockOnSleep,
    updateOptions,
    setOrChangePin,
    disablePin,
  } = useSecurityStore();

  const [privacyRules, setPrivacyRules] = useState<PrivacyRule[]>([]);
  const [newPattern, setNewPattern] = useState('');
  const [newRuleType, setNewRuleType] = useState<'private' | 'hidden'>('private');

  // PIN modal state
  const [pinModalMode, setPinModalMode] = useState<'set' | 'change' | 'disable' | null>(null);
  const [oldPinInput, setOldPinInput] = useState('');
  const [newPinInput, setNewPinInput] = useState('');
  const [confirmPinInput, setConfirmPinInput] = useState('');
  const [pinModalError, setPinModalError] = useState('');
  const [isSubmittingPin, setIsSubmittingPin] = useState(false);
  const [generatedRecoveryKey, setGeneratedRecoveryKey] = useState<string | null>(null);
  const [isCopied, setIsCopied] = useState(false);

  const loadRules = async () => {
    try {
      const list = await tauriApi.listPrivacyRules();
      setPrivacyRules(list);
    } catch (e) {
      console.error('Failed to load privacy rules:', e);
    }
  };

  useEffect(() => {
    loadRules();
  }, []);

  const handleOpenPinModal = (mode: 'set' | 'change' | 'disable') => {
    setPinModalMode(mode);
    setOldPinInput('');
    setNewPinInput('');
    setConfirmPinInput('');
    setPinModalError('');
  };

  const handleClosePinModal = () => {
    setPinModalMode(null);
    setPinModalError('');
  };

  const handlePinSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setPinModalError('');

    if (pinModalMode === 'disable') {
      if (!oldPinInput) {
        setPinModalError('请输入当前 PIN 码以确认停用');
        return;
      }
      setIsSubmittingPin(true);
      try {
        await disablePin(oldPinInput);
        showToast('应用锁已停用', 'info');
        handleClosePinModal();
      } catch (err: any) {
        setPinModalError(typeof err === 'string' ? err : '停用失败，当前 PIN 错误');
      } finally {
        setIsSubmittingPin(false);
      }
      return;
    }

    // Set or Change mode
    if (pinModalMode === 'change' && !oldPinInput) {
      setPinModalError('请输入原 PIN 码');
      return;
    }

    if (newPinInput.length < 4 || newPinInput.length > 6 || !/^\d+$/.test(newPinInput)) {
      setPinModalError('新 PIN 码必须为 4 至 6 位纯数字');
      return;
    }

    if (newPinInput !== confirmPinInput) {
      setPinModalError('两次输入的新 PIN 码不一致');
      return;
    }

    setIsSubmittingPin(true);
    try {
      const recKey = await setOrChangePin(pinModalMode === 'change' ? oldPinInput : null, newPinInput);
      showToast(pinModalMode === 'change' ? 'PIN 码修改成功' : '应用锁已成功启用', 'success');
      handleClosePinModal();
      if (recKey) {
        setGeneratedRecoveryKey(recKey);
      }
    } catch (err: any) {
      setPinModalError(typeof err === 'string' ? err : '设置失败');
    } finally {
      setIsSubmittingPin(false);
    }
  };

  const handleAddRule = async (e: React.FormEvent) => {
    e.preventDefault();
    const trimmed = newPattern.trim();
    if (!trimmed) return;

    try {
      await tauriApi.addPrivacyRule(trimmed, newRuleType);
      setNewPattern('');
      showToast(`已添加隐私规则: ${trimmed}`, 'success');
      await loadRules();
    } catch (err: any) {
      showToast(typeof err === 'string' ? err : '添加规则失败', 'error');
    }
  };

  const handleToggleRule = async (id: number, enabled: boolean) => {
    try {
      await tauriApi.togglePrivacyRule(id, enabled);
      setPrivacyRules((prev) =>
        prev.map((r) => (r.id === id ? { ...r, enabled } : r))
      );
    } catch (err) {
      console.error('Failed to toggle rule:', err);
    }
  };

  const handleDeleteRule = async (id: number) => {
    try {
      await tauriApi.deletePrivacyRule(id);
      setPrivacyRules((prev) => prev.filter((r) => r.id !== id));
      showToast('规则已删除', 'info');
    } catch (err) {
      console.error('Failed to delete rule:', err);
    }
  };

  return (
    <div className="bg-white dark:bg-slate-800 p-4 rounded-xl border border-slate-200 dark:border-slate-700 shadow-sm flex flex-col gap-4">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Shield className="w-4 h-4 text-indigo-600 dark:text-indigo-400" />
          <span className="font-semibold text-slate-800 dark:text-slate-100">
            隐私保护与应用锁 (Privacy & Security)
          </span>
        </div>
        <span
          className={`text-[10px] px-2 py-0.5 rounded-full font-medium ${
            pinEnabled
              ? 'bg-emerald-50 text-emerald-700 dark:bg-emerald-950/50 dark:text-emerald-300'
              : 'bg-slate-100 text-slate-500 dark:bg-slate-700 dark:text-slate-400'
          }`}
        >
          {pinEnabled ? 'PIN 锁已启用' : '未加锁'}
        </span>
      </div>

      {/* PIN Control Row */}
      <div className="p-3 bg-slate-50 dark:bg-slate-900/60 rounded-lg border border-slate-200/80 dark:border-slate-800 flex items-center justify-between">
        <div className="flex items-center gap-2.5">
          <Lock className="w-4 h-4 text-slate-500 dark:text-slate-400" />
          <div className="flex flex-col">
            <span className="font-medium text-slate-800 dark:text-slate-200">
              应用程序锁 (PIN Lock)
            </span>
            <span className="text-[11px] text-slate-400">
              {pinEnabled
                ? '已启用界面锁。锁定后主界面卸载；磁盘上的 archive.db 仍为明文 SQLite'
                : '开启后离开或锁屏需输入数字 PIN 才能看到界面。PIN 不加密数据库'}
            </span>
          </div>
        </div>

        <div className="flex items-center gap-2">
          {pinEnabled ? (
            <>
              <button
                type="button"
                onClick={() => handleOpenPinModal('change')}
                className="px-2.5 py-1.5 rounded bg-slate-200 dark:bg-slate-800 hover:bg-slate-300 dark:hover:bg-slate-700 text-slate-700 dark:text-slate-200 text-xs font-medium transition"
              >
                修改 PIN
              </button>
              <button
                type="button"
                onClick={() => handleOpenPinModal('disable')}
                className="px-2.5 py-1.5 rounded bg-rose-50 hover:bg-rose-100 dark:bg-rose-950/40 dark:hover:bg-rose-900/50 text-rose-600 dark:text-rose-400 text-xs font-medium transition"
              >
                停用
              </button>
            </>
          ) : (
            <button
              type="button"
              onClick={() => handleOpenPinModal('set')}
              className="px-3 py-1.5 rounded bg-indigo-600 hover:bg-indigo-700 text-white text-xs font-medium shadow-sm transition"
            >
              开启 PIN 保护
            </button>
          )}
        </div>
      </div>

      {/* Auto-Lock Options */}
      {pinEnabled && (
        <div className="grid grid-cols-1 sm:grid-cols-3 gap-3 p-3 bg-slate-50/50 dark:bg-slate-900/30 rounded-lg border border-slate-200/60 dark:border-slate-800/60 text-xs">
          <div className="flex flex-col gap-1">
            <label className="text-slate-600 dark:text-slate-400 font-medium">闲置自动锁定</label>
            <select
              value={autoLockMinutes}
              onChange={(e) =>
                updateOptions(Number(e.target.value), lockOnMinimize, lockOnSleep)
              }
              className="px-2 py-1.5 rounded border border-slate-300 dark:border-slate-700 bg-white dark:bg-slate-900 text-slate-800 dark:text-slate-200"
            >
              <option value="0">从不 (仅手动锁定)</option>
              <option value="5">5 分钟</option>
              <option value="15">15 分钟 (推荐)</option>
              <option value="30">30 分钟</option>
              <option value="60">1 小时</option>
            </select>
          </div>

          <div className="flex items-center gap-2 pt-4 sm:pt-0">
            <input
              type="checkbox"
              id="lock_minimize"
              checked={lockOnMinimize}
              onChange={(e) =>
                updateOptions(autoLockMinutes, e.target.checked, lockOnSleep)
              }
              className="w-4 h-4 rounded text-indigo-600 focus:ring-indigo-500 border-slate-300 dark:border-slate-700"
            />
            <label htmlFor="lock_minimize" className="text-slate-700 dark:text-slate-300 select-none">
              窗口最小化后锁定
            </label>
          </div>

          <div className="flex items-center gap-2 pt-4 sm:pt-0">
            <input
              type="checkbox"
              id="lock_sleep"
              checked={lockOnSleep}
              onChange={(e) =>
                updateOptions(autoLockMinutes, lockOnMinimize, e.target.checked)
              }
              className="w-4 h-4 rounded text-indigo-600 focus:ring-indigo-500 border-slate-300 dark:border-slate-700"
            />
            <label htmlFor="lock_sleep" className="text-slate-700 dark:text-slate-300 select-none">
              系统睡眠唤醒时锁定
            </label>
          </div>
        </div>
      )}

      {/* Privacy Rules List & Add */}
      <div className="flex flex-col gap-2 pt-2 border-t border-slate-100 dark:border-slate-800">
        <div className="flex items-center justify-between">
          <span className="font-semibold text-slate-700 dark:text-slate-200">
            隐私过滤规则 (Privacy Rules)
          </span>
          <span className="text-[11px] text-slate-400">
            Private 规则阻止发送至云端 AI；Hidden 规则完全脱敏排除
          </span>
        </div>

        {/* Existing Rules Table */}
        <div className="border border-slate-200 dark:border-slate-700 rounded-lg overflow-hidden">
          <table className="w-full text-left text-xs">
            <thead className="bg-slate-50 dark:bg-slate-900/60 text-slate-500 border-b border-slate-200 dark:border-slate-700">
              <tr>
                <th className="py-2 px-3">匹配模式 (Pattern)</th>
                <th className="py-2 px-3">规则级别</th>
                <th className="py-2 px-3">启用状态</th>
                <th className="py-2 px-3 text-right">操作</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-100 dark:divide-slate-800">
              {privacyRules.map((rule) => (
                <tr key={rule.id} className="hover:bg-slate-50/50 dark:hover:bg-slate-800/50">
                  <td className="py-2 px-3 font-mono text-slate-800 dark:text-slate-200">
                    {rule.pattern}
                  </td>
                  <td className="py-2 px-3">
                    <span
                      className={`inline-block text-[10px] px-1.5 py-0.5 rounded font-mono ${
                        rule.rule_type === 'hidden'
                          ? 'bg-purple-50 text-purple-700 dark:bg-purple-950/60 dark:text-purple-300'
                          : 'bg-blue-50 text-blue-700 dark:bg-blue-950/60 dark:text-blue-300'
                      }`}
                    >
                      {rule.rule_type === 'hidden' ? 'Hidden (完全排除)' : 'Private (免AI上传)'}
                    </span>
                  </td>
                  <td className="py-2 px-3">
                    <button
                      type="button"
                      onClick={() => handleToggleRule(rule.id, !rule.enabled)}
                      className={`text-[11px] font-medium px-2 py-0.5 rounded transition ${
                        rule.enabled
                          ? 'bg-emerald-50 text-emerald-600 dark:bg-emerald-950/60 dark:text-emerald-400'
                          : 'bg-slate-100 text-slate-400 dark:bg-slate-800'
                      }`}
                    >
                      {rule.enabled ? '已启用' : '已暂停'}
                    </button>
                  </td>
                  <td className="py-2 px-3 text-right">
                    <button
                      type="button"
                      onClick={() => handleDeleteRule(rule.id)}
                      className="p-1 text-slate-400 hover:text-rose-500 transition"
                      title="删除规则"
                    >
                      <Trash2 className="w-3.5 h-3.5" />
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>

        {/* Add New Rule Form */}
        <form onSubmit={handleAddRule} className="flex gap-2 items-center mt-1">
          <input
            type="text"
            placeholder="输入匹配域名或通配符，如 internal.corp 或 bank.com"
            value={newPattern}
            onChange={(e) => setNewPattern(e.target.value)}
            className="flex-1 px-2.5 py-1.5 rounded border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-900 text-slate-800 dark:text-slate-200 text-xs font-mono"
          />
          <select
            value={newRuleType}
            onChange={(e) => setNewRuleType(e.target.value as any)}
            className="px-2 py-1.5 rounded border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-900 text-slate-700 dark:text-slate-300 text-xs"
          >
            <option value="private">Private (禁止云AI)</option>
            <option value="hidden">Hidden (完全排除)</option>
          </select>
          <button
            type="submit"
            disabled={!newPattern.trim()}
            className="px-3 py-1.5 rounded bg-slate-800 hover:bg-slate-700 dark:bg-slate-700 dark:hover:bg-slate-600 text-white text-xs font-medium flex items-center gap-1 transition disabled:opacity-50"
          >
            <Plus className="w-3.5 h-3.5" />
            <span>添加</span>
          </button>
        </form>
      </div>

      {/* PIN Setup / Change Modal */}
      {pinModalMode && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/50 backdrop-blur-xs">
          <div className="w-full max-w-sm p-6 bg-white dark:bg-slate-900 border border-slate-200 dark:border-slate-800 rounded-2xl shadow-2xl animate-in zoom-in-95">
            <div className="flex items-center justify-between mb-4">
              <div className="flex items-center gap-2">
                <KeyRound className="w-5 h-5 text-indigo-600" />
                <h3 className="font-bold text-sm text-slate-900 dark:text-slate-100">
                  {pinModalMode === 'set'
                    ? '设置应用锁 PIN'
                    : pinModalMode === 'change'
                    ? '修改应用锁 PIN'
                    : '停用应用锁'}
                </h3>
              </div>
              <button onClick={handleClosePinModal} className="p-1 text-slate-400 hover:text-slate-600">
                <X className="w-4 h-4" />
              </button>
            </div>

            <form onSubmit={handlePinSubmit} className="flex flex-col gap-3">
              {pinModalMode !== 'set' && (
                <div className="flex flex-col gap-1">
                  <label className="text-slate-500 font-medium">当前 PIN 码</label>
                  <input
                    type="password"
                    maxLength={6}
                    autoFocus
                    placeholder="输入当前 4~6 位 PIN"
                    value={oldPinInput}
                    onChange={(e) => setOldPinInput(e.target.value)}
                    className="px-3 py-2 rounded-lg border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-800 font-mono tracking-widest text-center"
                  />
                </div>
              )}

              {pinModalMode !== 'disable' && (
                <>
                  <div className="flex flex-col gap-1">
                    <label className="text-slate-500 font-medium">新 PIN 码 (4~6 位纯数字)</label>
                    <input
                      type="password"
                      maxLength={6}
                      placeholder="输入 4~6 位纯数字"
                      value={newPinInput}
                      onChange={(e) => setNewPinInput(e.target.value)}
                      className="px-3 py-2 rounded-lg border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-800 font-mono tracking-widest text-center"
                    />
                  </div>

                  <div className="flex flex-col gap-1">
                    <label className="text-slate-500 font-medium">再次确认新 PIN 码</label>
                    <input
                      type="password"
                      maxLength={6}
                      placeholder="再次输入新 PIN"
                      value={confirmPinInput}
                      onChange={(e) => setConfirmPinInput(e.target.value)}
                      className="px-3 py-2 rounded-lg border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-800 font-mono tracking-widest text-center"
                    />
                  </div>
                </>
              )}

              {pinModalError && (
                <div className="text-rose-500 text-xs flex items-center gap-1.5 mt-1">
                  <AlertCircle className="w-3.5 h-3.5 flex-shrink-0" />
                  <span>{pinModalError}</span>
                </div>
              )}

              <div className="flex justify-end gap-2 mt-4">
                <button
                  type="button"
                  onClick={handleClosePinModal}
                  className="px-3 py-1.5 rounded-lg border border-slate-300 dark:border-slate-700 text-slate-600 dark:text-slate-300 hover:bg-slate-100 dark:hover:bg-slate-800"
                >
                  取消
                </button>
                <button
                  type="submit"
                  disabled={isSubmittingPin}
                  className="px-4 py-1.5 rounded-lg bg-indigo-600 hover:bg-indigo-700 text-white font-medium"
                >
                  {isSubmittingPin ? '处理中...' : '确认'}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      {/* Recovery Key Presentation Modal */}
      {generatedRecoveryKey && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/60 backdrop-blur-sm p-4 animate-in fade-in">
          <div className="w-full max-w-md bg-white dark:bg-slate-800 rounded-2xl border border-slate-200 dark:border-slate-700 shadow-2xl p-6 flex flex-col gap-4">
            <div className="flex items-center gap-3">
              <div className="w-10 h-10 rounded-xl bg-amber-50 dark:bg-amber-950/60 border border-amber-200 dark:border-amber-800/60 flex items-center justify-center text-amber-600 dark:text-amber-400 shadow-inner">
                <KeyRound className="w-5 h-5" />
              </div>
              <div>
                <h3 className="font-bold text-base text-slate-900 dark:text-slate-100">
                  安全恢复密钥 (Recovery Key)
                </h3>
                <p className="text-xs text-slate-500">
                  用于忘记 PIN 时重置应用锁，不会加密历史数据库
                </p>
              </div>
            </div>

            <p className="text-xs text-slate-600 dark:text-slate-300 leading-relaxed">
              请把这串恢复密钥存到安全的地方。忘记 PIN 时可在锁屏用它重置应用锁。历史记录保存在本机明文数据库中，关闭应用锁或直接打开数据目录仍可读取。
            </p>

            <div className="p-3.5 bg-slate-100 dark:bg-slate-900 rounded-xl border border-slate-200 dark:border-slate-700/80 font-mono text-xs text-slate-800 dark:text-slate-100 break-all select-all flex items-center justify-between gap-2">
              <span className="font-semibold tracking-wider">{generatedRecoveryKey}</span>
              <button
                type="button"
                onClick={() => {
                  navigator.clipboard.writeText(generatedRecoveryKey);
                  setIsCopied(true);
                  setTimeout(() => setIsCopied(false), 2000);
                  showToast('恢复密钥已复制到剪贴板', 'info');
                }}
                className="p-1.5 rounded-lg hover:bg-slate-200 dark:hover:bg-slate-800 text-slate-500 hover:text-slate-700 dark:hover:text-slate-200 transition cursor-pointer"
                title="复制"
              >
                {isCopied ? <Check className="w-4 h-4 text-emerald-500" /> : <Copy className="w-4 h-4" />}
              </button>
            </div>

            <div className="flex items-center justify-between pt-2">
              <button
                type="button"
                onClick={() => {
                  const blob = new Blob([`Browsory Recovery Key:\n${generatedRecoveryKey}\n\nGenerated at: ${new Date().toISOString()}`], { type: 'text/plain' });
                  const url = URL.createObjectURL(blob);
                  const a = document.createElement('a');
                  a.href = url;
                  a.download = `browsory-recovery-key-${Date.now()}.txt`;
                  a.click();
                  URL.revokeObjectURL(url);
                  showToast('已下载恢复密钥 TXT 文件', 'success');
                }}
                className="px-3 py-1.5 rounded-lg border border-slate-300 dark:border-slate-700 hover:bg-slate-100 dark:hover:bg-slate-700 text-slate-700 dark:text-slate-200 text-xs flex items-center gap-1.5 transition cursor-pointer"
              >
                <FileText className="w-3.5 h-3.5 text-blue-500" />
                <span>保存为文本文件</span>
              </button>

              <button
                type="button"
                onClick={() => setGeneratedRecoveryKey(null)}
                className="px-4 py-1.5 rounded-lg bg-indigo-600 hover:bg-indigo-700 text-white text-xs font-semibold shadow-md transition cursor-pointer"
              >
                我已妥善保存
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};
