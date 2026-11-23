import React, { useState, useEffect, useRef } from 'react';
import {
  ShieldCheck,
  Lock,
  EyeOff,
  Cpu,
  Database,
  Cloud,
  RefreshCw,
  Plus,
  Trash2,
  ChevronDown,
  ChevronUp,
  HardDrive,
} from 'lucide-react';
import { useAppStore } from '../../stores/useAppStore';
import { useSecurityStore } from '../../stores/useSecurityStore';
import { tauriApi } from '../../services/tauri';
import { PrivacyRule } from '../../types';
import { createRequestSequence } from '../../services/requestSequence';
import { SecuritySettingsCard } from '../Settings/SecuritySettingsCard';
import { MaintenanceSettingsCard } from '../Settings/MaintenanceSettingsCard';

export const PrivacyPage: React.FC = () => {
  const { showToast, t, language } = useAppStore();
  const { lockVault, pinEnabled, hasDpapiWrap } = useSecurityStore();
  const ui = (zh: string, en: string) => (language === 'en-US' ? en : zh);

  const [rulesExpanded, setRulesExpanded] = useState(false);
  const [securityExpanded, setSecurityExpanded] = useState(true);
  const [maintenanceExpanded, setMaintenanceExpanded] = useState(false);

  const [rules, setRules] = useState<PrivacyRule[]>([]);
  const [loadingRules, setLoadingRules] = useState(false);
  const [rulesError, setRulesError] = useState<string | null>(null);
  const [pendingRuleIds, setPendingRuleIds] = useState<Set<number>>(new Set());
  const rulesSequence = useRef(createRequestSequence());
  const [newPattern, setNewPattern] = useState('');
  const [newRuleType, setNewRuleType] = useState<'private' | 'hidden'>('private');
  const [creatingRule, setCreatingRule] = useState(false);
  const [confirmDeleteId, setConfirmDeleteId] = useState<number | null>(null);

  const fetchRules = async () => {
    const request = rulesSequence.current.next();
    setLoadingRules(true);
    setRulesError(null);
    try {
      const data = await tauriApi.listPrivacyRules();
      if (rulesSequence.current.isCurrent(request)) setRules(data);
    } catch (err) {
      if (rulesSequence.current.isCurrent(request)) {
        setRulesError(
          typeof err === 'string'
            ? err
            : ui('无法读取隐私规则，请重试；以下内容可能尚未更新。', 'Unable to read privacy rules. Retry; the list may be out of date.')
        );
      }
    } finally {
      if (rulesSequence.current.isCurrent(request)) setLoadingRules(false);
    }
  };

  useEffect(() => {
    void fetchRules();
    return () => {
      rulesSequence.current.next();
    };
  }, []);

  const handleAddRule = async () => {
    if (!newPattern.trim()) {
      showToast(ui('请输入要屏蔽的域名或路径规则', 'Enter a domain or path rule to block'), 'error');
      return;
    }
    try {
      setCreatingRule(true);
      await tauriApi.addPrivacyRule(newPattern.trim(), newRuleType);
      showToast(ui('隐私保护规则已添加', 'Privacy rule added'), 'success');
      setNewPattern('');
      fetchRules();
    } catch {
      showToast(ui('添加规则失败', 'Failed to add rule'), 'error');
    } finally {
      setCreatingRule(false);
    }
  };

  const handleToggleRule = async (id: number, currentEnabled: boolean) => {
    setPendingRuleIds((current) => new Set(current).add(id));
    try {
      await tauriApi.togglePrivacyRule(id, !currentEnabled);
      fetchRules();
    } catch (err) {
      showToast(
        typeof err === 'string' ? err : ui('更改规则失败，原设置未更新', 'Unable to change the rule. Its previous setting is retained.'),
        'error'
      );
    } finally {
      setPendingRuleIds((current) => {
        const next = new Set(current);
        next.delete(id);
        return next;
      });
    }
  };

  const handleDeleteRule = async (id: number) => {
    setConfirmDeleteId(null);
    setPendingRuleIds((current) => new Set(current).add(id));
    try {
      await tauriApi.deletePrivacyRule(id);
      showToast(ui('已移除该隐私规则', 'Privacy rule removed'), 'success');
      fetchRules();
    } catch (err) {
      showToast(typeof err === 'string' ? err : ui('删除规则失败，请重试', 'Unable to delete the rule. Please retry.'), 'error');
    } finally {
      setPendingRuleIds((current) => {
        const next = new Set(current);
        next.delete(id);
        return next;
      });
    }
  };

  const handleLockVault = async () => {
    if (hasDpapiWrap && !pinEnabled) {
      showToast(
        ui('已启用后台静默加密；请先在下方设置 4 位 PIN 码以使用手动锁屏', 'Background encryption is active; set a 4-digit PIN below first to lock manually'),
        'info'
      );
      setSecurityExpanded(true);
      return;
    }
    try {
      await lockVault();
    } catch (err: any) {
      showToast(err?.message || ui('锁定保险库失败', 'Failed to lock the vault'), 'error');
    }
  };

  return (
    <div className="flex flex-col h-full bg-slate-50 dark:bg-slate-950 overflow-y-auto">
      {/* Compact Desktop Header */}
      <div className="px-6 py-3.5 bg-white dark:bg-slate-900 border-b border-slate-200 dark:border-slate-800 shrink-0">
        <div className="max-w-6xl mx-auto flex flex-col sm:flex-row sm:items-center justify-between gap-3">
          <div className="flex items-center gap-3">
            <div className="p-2 rounded-xl bg-blue-600/10 text-blue-600 dark:text-blue-400 border border-blue-600/20">
              <ShieldCheck className="w-5 h-5" />
            </div>
            <div>
              <div className="flex items-center gap-2">
                <h1 className="text-base font-bold text-slate-900 dark:text-slate-100">
                  {t('privacy.title')}
                </h1>
                <span className="px-2 py-0.5 text-[11px] font-medium rounded-full bg-emerald-100 dark:bg-emerald-950/80 text-emerald-700 dark:text-emerald-300 border border-emerald-300 dark:border-emerald-800">
                  {ui('本地存储保障', 'Local storage protected')}
                </span>
              </div>
              <p className="text-xs text-slate-500 dark:text-slate-400 mt-0.5">
                {ui(
                  `${t('privacy.subtitle')} 云端 AI 仅在你主动配置后才会出网。`,
                  `${t('privacy.subtitle')} Cloud AI connects only when you configure and invoke it.`
                )}
              </p>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={handleLockVault}
              className="flex items-center gap-1.5 px-3.5 py-1.5 text-xs font-medium rounded-xl bg-slate-100 dark:bg-slate-800 hover:bg-slate-200 dark:hover:bg-slate-700 text-slate-700 dark:text-slate-200 transition cursor-pointer"
            >
              <Lock className="w-3.5 h-3.5 text-amber-500" />
              <span>{t('privacy.lockVault')}</span>
            </button>
          </div>
        </div>
      </div>

      {/* Main Content Area: Collapsible security & privacy panels */}
      <div className="max-w-6xl mx-auto w-full p-6 space-y-5">
        {/* 1. Privacy Rules & Excluded Domains Engine */}
        <div className="rounded-2xl border border-slate-200 dark:border-slate-800 bg-white dark:bg-slate-900 shadow-xs overflow-hidden">
          <div className="px-5 py-4 flex items-center justify-between gap-3">
            <button
              type="button"
              onClick={() => setRulesExpanded((prev) => !prev)}
              aria-expanded={rulesExpanded}
              className="flex-1 flex items-center justify-between gap-3 text-left cursor-pointer"
            >
              <div className="flex items-center gap-3 min-w-0">
                <div className="p-2 rounded-xl bg-rose-50 dark:bg-rose-950/60 text-rose-600 dark:text-rose-400 border border-rose-200 dark:border-rose-900/40 shrink-0">
                  <EyeOff className="w-4 h-4" />
                </div>
                <div className="min-w-0">
                  <div className="flex items-center gap-2 flex-wrap">
                    <h2 className="text-sm font-semibold text-slate-900 dark:text-slate-100">
                      {t('privacy.rulesTitle')}
                    </h2>
                    <span className="text-[11px] px-2 py-0.5 rounded-full bg-slate-100 dark:bg-slate-800 text-slate-600 dark:text-slate-300 font-medium">
                      {ui(`${rules.length} 条规则`, `${rules.length} rules`)}
                    </span>
                  </div>
                  <p className="text-xs text-slate-500 dark:text-slate-400 mt-0.5 truncate">
                    {t('privacy.rulesDescription')}
                  </p>
                </div>
              </div>
              <span className="p-1 text-slate-400 shrink-0">
                {rulesExpanded ? <ChevronUp className="w-4 h-4" /> : <ChevronDown className="w-4 h-4" />}
              </span>
            </button>

            <button
              type="button"
              onClick={fetchRules}
              disabled={loadingRules}
              aria-label={ui('刷新规则', 'Refresh rules')}
              className="p-1.5 rounded-lg text-slate-400 hover:text-slate-600 dark:hover:text-slate-200 hover:bg-slate-100 dark:hover:bg-slate-800 transition cursor-pointer shrink-0"
              title={ui('刷新规则', 'Refresh rules')}
            >
              <RefreshCw className={`w-4 h-4 ${loadingRules ? 'animate-spin' : ''}`} />
            </button>
          </div>

          {rulesExpanded && (
            <div className="px-5 pb-5 pt-2 border-t border-slate-100 dark:border-slate-800 space-y-4">
              {/* Add New Rule Form */}
              <div className="p-4 rounded-xl bg-slate-50 dark:bg-slate-800/60 border border-slate-200/80 dark:border-slate-800 flex flex-col md:flex-row items-end gap-3">
                <div className="flex-1 w-full flex flex-col gap-1.5">
                  <label className="text-xs font-medium text-slate-600 dark:text-slate-300">
                    {ui('匹配模式 / 域名 / 路径关键字', 'Pattern / domain / path keyword')}
                  </label>
                  <p className="text-xs text-slate-500 dark:text-slate-400">
                    {t('privacy.matchHelp')}
                  </p>
                  <input
                    type="text"
                    value={newPattern}
                    onChange={(e) => setNewPattern(e.target.value)}
                    placeholder={ui('例如: mail.google.com 或 bank 或 localhost', 'e.g. mail.google.com, bank, or localhost')}
                    className="w-full px-3 py-2 text-xs rounded-xl border border-slate-200 dark:border-slate-800 bg-white dark:bg-slate-900 font-mono text-slate-800 dark:text-slate-200 focus:outline-hidden focus:ring-1 focus:ring-blue-500"
                  />
                </div>

                <div className="w-full md:w-48 flex flex-col gap-1.5">
                  <label className="text-xs font-medium text-slate-600 dark:text-slate-300">
                    {ui('保护级别', 'Protection level')}
                  </label>
                  <select
                    value={newRuleType}
                    onChange={(e) => setNewRuleType(e.target.value as 'private' | 'hidden')}
                    className="w-full px-3 py-2 text-xs rounded-xl border border-slate-200 dark:border-slate-800 bg-white dark:bg-slate-900 text-slate-800 dark:text-slate-200 focus:outline-hidden focus:ring-1 focus:ring-blue-500"
                  >
                    <option value="private">{t('privacy.privateOption')}</option>
                    <option value="hidden">{t('privacy.hiddenOption')}</option>
                  </select>
                </div>

                <button
                  type="button"
                  onClick={handleAddRule}
                  disabled={creatingRule}
                  className="w-full md:w-auto px-4 py-2 rounded-xl bg-blue-600 hover:bg-blue-700 disabled:opacity-50 text-white text-xs font-medium transition flex items-center justify-center gap-1.5 cursor-pointer shadow-xs"
                >
                  <Plus className="w-3.5 h-3.5" />
                  <span>{creatingRule ? ui('添加中...', 'Adding...') : ui('添加规则', 'Add rule')}</span>
                </button>
              </div>

              {rulesError && (
                <div role="alert" className="flex items-start justify-between gap-3 rounded border border-rose-200 bg-rose-50 p-3 text-xs text-rose-700 dark:border-rose-900 dark:bg-rose-950/30 dark:text-rose-300">
                  <span>{rulesError}</span>
                  <button type="button" onClick={fetchRules} disabled={loadingRules} className="shrink-0 underline disabled:opacity-50 cursor-pointer">{t('common.retry')}</button>
                </div>
              )}

              {/* Rules List */}
              <div className="space-y-2" aria-busy={loadingRules}>
                {rules.length === 0 && loadingRules ? (
                  <div role="status" className="p-5 text-center text-xs text-slate-500">{t('history.loading')}</div>
                ) : rules.length === 0 && rulesError ? null : rules.length === 0 ? (
                  <div className="p-6 text-center text-slate-500 dark:text-slate-400 text-xs">
                    {ui(
                      '尚未配置任何自定义隐私规则。内置 ip:non-public 规则按存储 URL 中的 IP 字面量覆盖非公开 IPv4/IPv6（不做 DNS 推断）；AI 脱敏是有限规则，不等于全面匿名化。',
                      'No custom privacy rules are configured. The built-in ip:non-public rule matches literal private IPv4/IPv6 addresses in stored URLs without DNS inference; AI redaction is limited, not full anonymization.'
                    )}
                  </div>
                ) : (
                  rules.map((rule) => (
                    <div
                      key={rule.id}
                      className="flex items-center justify-between p-3 rounded-xl border border-slate-100 dark:border-slate-800 bg-slate-50/50 dark:bg-slate-900/50 hover:bg-slate-50 dark:hover:bg-slate-800/80 transition"
                    >
                      <div className="flex items-center gap-3">
                        <span className="font-mono text-xs font-medium text-slate-800 dark:text-slate-200">
                          {rule.pattern}
                        </span>
                        <span
                          className={`px-2 py-0.5 rounded text-[11px] uppercase font-mono ${
                            rule.rule_type === 'private'
                              ? 'bg-rose-100 dark:bg-rose-950/80 text-rose-700 dark:text-rose-300'
                              : 'bg-blue-100 dark:bg-blue-950/80 text-blue-700 dark:text-blue-300'
                          }`}
                        >
                          {rule.rule_type === 'private' ? ui('Private · 仅排除 AI', 'Private · AI only') : ui('Hidden · 全局排除', 'Hidden · exclude everywhere')}
                        </span>
                      </div>

                      <div className="flex items-center gap-3">
                        <label className="relative inline-flex items-center cursor-pointer">
                          <input
                            type="checkbox"
                            checked={rule.enabled}
                            disabled={pendingRuleIds.has(rule.id)}
                            aria-label={`${ui('启用规则', 'Enable rule')}: ${rule.pattern}`}
                            onChange={() => handleToggleRule(rule.id, rule.enabled)}
                            className="sr-only peer"
                          />
                          <div className="w-8 h-4.5 bg-slate-200 peer-focus:outline-hidden rounded-full peer dark:bg-slate-700 peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-slate-300 after:border after:rounded-full after:h-3.5 after:w-3.5 after:transition-all dark:border-slate-600 peer-checked:bg-blue-600"></div>
                        </label>
                        {confirmDeleteId === rule.id ? (
                          <div className="flex items-center gap-1.5">
                            <button
                              type="button"
                              onClick={() => handleDeleteRule(rule.id)}
                              disabled={pendingRuleIds.has(rule.id)}
                              className="px-2 py-1 rounded-md bg-rose-600 hover:bg-rose-700 disabled:opacity-50 text-white text-xs font-medium cursor-pointer"
                            >
                              {ui('确认删除', 'Confirm')}
                            </button>
                            <button
                              type="button"
                              onClick={() => setConfirmDeleteId(null)}
                              className="px-2 py-1 rounded-md border border-slate-300 dark:border-slate-700 text-slate-600 dark:text-slate-300 hover:bg-slate-100 dark:hover:bg-slate-800 text-xs cursor-pointer"
                            >
                              {ui('取消', 'Cancel')}
                            </button>
                          </div>
                        ) : (
                          <button
                            type="button"
                            onClick={() => setConfirmDeleteId(rule.id)}
                            disabled={pendingRuleIds.has(rule.id)}
                            className="p-1 text-slate-400 hover:text-red-600 dark:hover:text-red-400 transition cursor-pointer disabled:opacity-50"
                            aria-label={`${ui('删除规则', 'Delete rule')}: ${rule.pattern}`}
                            title={ui('删除规则', 'Delete rule')}
                          >
                            <Trash2 className="w-4 h-4" />
                          </button>
                        )}
                      </div>
                    </div>
                  ))
                )}
              </div>
            </div>
          )}
        </div>

        {/* 2. PIN Lock & Security Settings */}
        <div className="rounded-2xl border border-slate-200 dark:border-slate-800 bg-white dark:bg-slate-900 shadow-xs overflow-hidden">
          <button
            type="button"
            onClick={() => setSecurityExpanded((prev) => !prev)}
            aria-expanded={securityExpanded}
            className="w-full px-5 py-4 flex items-center justify-between gap-3 text-left cursor-pointer hover:bg-slate-50/70 dark:hover:bg-slate-800/40 transition-colors"
          >
            <div className="flex items-center gap-3 min-w-0">
              <div className="p-2 rounded-xl bg-blue-50 dark:bg-blue-950/60 text-blue-600 dark:text-blue-400 border border-blue-200 dark:border-blue-900/40 shrink-0">
                <Lock className="w-4 h-4" />
              </div>
              <div className="min-w-0">
                <div className="flex items-center gap-2 flex-wrap">
                  <h2 className="text-sm font-semibold text-slate-900 dark:text-slate-100">
                    {ui('应用访问锁与安全保护', 'App access lock and security')}
                  </h2>
                  <span
                    className={`text-[11px] px-2 py-0.5 rounded-full font-medium ${
                      pinEnabled
                        ? 'bg-emerald-50 text-emerald-700 dark:bg-emerald-950/60 dark:text-emerald-300'
                        : 'bg-slate-100 text-slate-500 dark:bg-slate-800 dark:text-slate-400'
                    }`}
                  >
                    {pinEnabled ? ui('4 位 PIN 已启用', '4-digit PIN active') : ui('后台静默加密中', 'Silent encryption active')}
                  </span>
                </div>
                <p className="text-xs text-slate-500 dark:text-slate-400 mt-0.5">
                  {ui('管理本地数据库加密状态、按需 4 位 PIN 锁与自动锁定策略', 'Manage local database encryption, optional 4-digit PIN, and auto-lock policies')}
                </p>
              </div>
            </div>
            <span className="p-1 text-slate-400 shrink-0">
              {securityExpanded ? <ChevronUp className="w-4 h-4" /> : <ChevronDown className="w-4 h-4" />}
            </span>
          </button>

          {securityExpanded && (
            <div className="px-5 pb-5 pt-2 border-t border-slate-100 dark:border-slate-800">
              <SecuritySettingsCard />
            </div>
          )}
        </div>

        {/* 3. Automated Backup & Maintenance */}
        <div className="rounded-2xl border border-slate-200 dark:border-slate-800 bg-white dark:bg-slate-900 shadow-xs overflow-hidden">
          <button
            type="button"
            onClick={() => setMaintenanceExpanded((prev) => !prev)}
            aria-expanded={maintenanceExpanded}
            className="w-full px-5 py-4 flex items-center justify-between gap-3 text-left cursor-pointer hover:bg-slate-50/70 dark:hover:bg-slate-800/40 transition-colors"
          >
            <div className="flex items-center gap-3 min-w-0">
              <div className="p-2 rounded-xl bg-emerald-50 dark:bg-emerald-950/60 text-emerald-600 dark:text-emerald-400 border border-emerald-200 dark:border-emerald-900/40 shrink-0">
                <HardDrive className="w-4 h-4" />
              </div>
              <div className="min-w-0">
                <h2 className="text-sm font-semibold text-slate-900 dark:text-slate-100">
                  {ui('数据备份与数据库维护', 'Data backup and database maintenance')}
                </h2>
                <p className="text-xs text-slate-500 dark:text-slate-400 mt-0.5">
                  {ui('加密备份生成与恢复、数据库完整性体检、压缩整理与旧明文副本清理', 'Encrypted backups, integrity checks, database vacuum, and legacy cleanup')}
                </p>
              </div>
            </div>
            <span className="p-1 text-slate-400 shrink-0">
              {maintenanceExpanded ? <ChevronUp className="w-4 h-4" /> : <ChevronDown className="w-4 h-4" />}
            </span>
          </button>

          {maintenanceExpanded && (
            <div className="px-5 pb-5 pt-2 border-t border-slate-100 dark:border-slate-800">
              <MaintenanceSettingsCard />
            </div>
          )}
        </div>

        {/* 4. Collapsible Architecture & Uninstall Transparency */}
        <details className="rounded-2xl border border-slate-200 dark:border-slate-800 bg-white dark:bg-slate-900 p-4 text-xs">
          <summary className="cursor-pointer font-semibold text-slate-700 dark:text-slate-200 select-none">
            {ui('本地数据架构、隐私边界与卸载删除说明', 'Local data boundaries, AI redaction scope & uninstall notes')}
          </summary>
          <div className="mt-4 space-y-4">
            <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
              <div className="p-4 rounded-xl border border-slate-200 dark:border-slate-800 bg-slate-50/50 dark:bg-slate-950/40 flex flex-col justify-between">
                <div>
                  <div className="flex items-center gap-2 text-blue-600 dark:text-blue-400 mb-1.5">
                    <Database className="w-4 h-4" />
                    <span className="text-xs font-bold">{ui('100% 本地优先存储', '100% local-first storage')}</span>
                  </div>
                  <p className="text-xs text-slate-500 dark:text-slate-400 leading-relaxed">
                    {ui(
                      '浏览足迹保存在本机 SQLCipher 加密库中。Browsory 没有自有用户云；AI、WebDAV 和检查更新只会在对应功能触发时访问配置端点。',
                      'Browsing history stays in the local SQLCipher vault. Browsory operates no cloud service; external calls occur only when you configure AI, WebDAV, or check updates.'
                    )}
                  </p>
                </div>
                <div className="mt-3 pt-2.5 border-t border-slate-200/60 dark:border-slate-800 flex items-center justify-between text-[11px] text-slate-400">
                  <span>{ui('存储: 本地独占', 'Storage: local only')}</span>
                  <span className="text-emerald-500 font-medium">{ui('默认不出网', 'Offline by default')}</span>
                </div>
              </div>

              <div className="p-4 rounded-xl border border-slate-200 dark:border-slate-800 bg-slate-50/50 dark:bg-slate-950/40 flex flex-col justify-between">
                <div>
                  <div className="flex items-center gap-2 text-blue-600 dark:text-blue-400 mb-1.5">
                    <Cpu className="w-4 h-4" />
                    <span className="text-xs font-bold">{ui('AI 隐私管道脱敏', 'AI privacy redaction')}</span>
                  </div>
                  <p className="text-xs text-slate-500 dark:text-slate-400 leading-relaxed">
                    {ui(
                      '支持本地 Ollama 模型；选用云端模型时，只有主动请求才会发送所选记录，并对常见 Token、邮箱与敏感 URL 参数做有限遮盖（不承诺全面匿名化）。',
                      'Supports local Ollama endpoints. Cloud requests transmit only user-triggered context after excluding matching privacy rules and applying limited token/parameter redaction.'
                    )}
                  </p>
                </div>
                <div className="mt-3 pt-2.5 border-t border-slate-200/60 dark:border-slate-800 flex items-center justify-between text-[11px] text-slate-400">
                  <span>{ui('规则拦截: SQL 前置过滤', 'Rule gate: SQL pre-filter')}</span>
                  <span className="text-blue-500 font-medium">{ui('有限脱敏', 'Limited redaction')}</span>
                </div>
              </div>

              <div className="p-4 rounded-xl border border-slate-200 dark:border-slate-800 bg-slate-50/50 dark:bg-slate-950/40 flex flex-col justify-between">
                <div>
                  <div className="flex items-center gap-2 text-emerald-600 dark:text-emerald-400 mb-1.5">
                    <Cloud className="w-4 h-4" />
                    <span className="text-xs font-bold">{ui('WebDAV 隐私边界', 'WebDAV privacy boundary')}</span>
                  </div>
                  <p className="text-xs text-slate-500 dark:text-slate-400 leading-relaxed">
                    {ui(
                      'WebDAV 连接强制 HTTPS（回环地址除外），凭据存放在系统安全存储中。双向同步尚未开放，不会虚报同步完成。',
                      'WebDAV requires HTTPS outside loopback, with credentials in OS secure storage. Bidirectional sync remains disabled until full conflict handling ships.'
                    )}
                  </p>
                </div>
                <div className="mt-3 pt-2.5 border-t border-slate-200/60 dark:border-slate-800 flex items-center justify-between text-[11px] text-slate-400">
                  <span>{ui('状态: 双向同步未开放', 'Status: sync disabled')}</span>
                  <span className="text-amber-500 font-medium">Fail-closed</span>
                </div>
              </div>
            </div>

            <div className="p-3.5 rounded-xl bg-slate-50 dark:bg-slate-950/40 border border-slate-200/70 dark:border-slate-800 text-xs text-slate-600 dark:text-slate-300 leading-relaxed">
              <strong className="text-slate-800 dark:text-slate-100">{ui('卸载与数据删除：', 'Uninstalling & deleting data: ')}</strong>
              {ui(
                '卸载安装包通常不会删除历史库。请到设置页打开数据目录，自行决定是否删除 archive.db、backups 和 logs。完整说明见仓库 docs/PRIVACY.md，第三方许可见 docs/THIRD_PARTY_NOTICES.md。',
                'Uninstalling the app preserves your local vault data. Open the data folder from Settings if you wish to remove archive.db, backups, or logs. See docs/PRIVACY.md and docs/THIRD_PARTY_NOTICES.md.'
              )}
            </div>
          </div>
        </details>
      </div>
    </div>
  );
};
