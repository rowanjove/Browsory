import React, { useState, useEffect } from 'react';
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
} from 'lucide-react';
import { useAppStore } from '../../stores/useAppStore';
import { useSecurityStore } from '../../stores/useSecurityStore';
import { tauriApi } from '../../services/tauri';
import { PrivacyRule } from '../../types';
import { SecuritySettingsCard } from '../Settings/SecuritySettingsCard';
import { MaintenanceSettingsCard } from '../Settings/MaintenanceSettingsCard';

export const PrivacyPage: React.FC = () => {
  const { showToast } = useAppStore();
  const { pinEnabled, lock } = useSecurityStore();

  const [rules, setRules] = useState<PrivacyRule[]>([]);
  const [loadingRules, setLoadingRules] = useState(false);
  const [newPattern, setNewPattern] = useState('');
  const [newRuleType, setNewRuleType] = useState<'private' | 'hidden'>('private');
  const [creatingRule, setCreatingRule] = useState(false);

  // 加载隐私规则
  const fetchRules = async () => {
    try {
      setLoadingRules(true);
      const data = await tauriApi.listPrivacyRules();
      setRules(data);
    } catch (err) {
      console.error('Failed to load privacy rules:', err);
    } finally {
      setLoadingRules(false);
    }
  };

  useEffect(() => {
    fetchRules();
  }, []);

  const handleAddRule = async () => {
    if (!newPattern.trim()) {
      showToast('请输入要屏蔽的域名或路径规则', 'error');
      return;
    }
    try {
      setCreatingRule(true);
      await tauriApi.addPrivacyRule(newPattern.trim(), newRuleType);
      showToast('隐私保护规则已添加', 'success');
      setNewPattern('');
      fetchRules();
    } catch (err: any) {
      showToast(typeof err === 'string' ? err : '添加规则失败', 'error');
    } finally {
      setCreatingRule(false);
    }
  };

  const handleToggleRule = async (id: number, currentEnabled: boolean) => {
    try {
      await tauriApi.togglePrivacyRule(id, !currentEnabled);
      fetchRules();
    } catch (err) {
      console.error('Failed to toggle rule:', err);
    }
  };

  const handleDeleteRule = async (id: number) => {
    if (window.confirm('确定要删除这条隐私规则吗？')) {
      try {
        await tauriApi.deletePrivacyRule(id);
        showToast('已移除该隐私规则', 'success');
        fetchRules();
      } catch (err) {
        console.error('Failed to delete rule:', err);
      }
    }
  };

  return (
    <div className="flex flex-col h-full bg-slate-50 dark:bg-slate-950 overflow-y-auto">
      {/* Top Banner Header */}
      <div className="p-6 md:p-8 bg-white dark:bg-slate-900 border-b border-slate-200 dark:border-slate-800">
        <div className="max-w-6xl mx-auto flex flex-col md:flex-row md:items-center justify-between gap-4">
          <div className="flex items-center gap-4">
            <div className="p-3.5 rounded-2xl bg-gradient-to-br from-blue-600 to-indigo-700 text-white shadow-lg shadow-blue-500/20">
              <ShieldCheck className="w-8 h-8" />
            </div>
            <div>
              <div className="flex items-center gap-2.5">
                <h1 className="text-xl md:text-2xl font-bold text-slate-900 dark:text-slate-100">
                  隐私与安全中心
                </h1>
                <span className="px-2.5 py-0.5 text-xs font-semibold rounded-full bg-emerald-100 dark:bg-emerald-950/80 text-emerald-700 dark:text-emerald-300 border border-emerald-300 dark:border-emerald-800">
                  本地存储保障
                </span>
              </div>
              <p className="text-xs md:text-sm text-slate-500 dark:text-slate-400 mt-1">
                明确数据仅保存在本地、禁止隐蔽联网、支持端到端加密与访问锁。
              </p>
            </div>
          </div>

          <div className="flex items-center gap-2">
            {pinEnabled && (
              <button
                onClick={lock}
                className="flex items-center gap-1.5 px-4 py-2 text-xs font-medium rounded-xl bg-slate-100 dark:bg-slate-800 hover:bg-slate-200 dark:hover:bg-slate-700 text-slate-700 dark:text-slate-200 transition cursor-pointer"
              >
                <Lock className="w-3.5 h-3.5 text-amber-500" />
                <span>立即锁定当前窗口</span>
              </button>
            )}
          </div>
        </div>
      </div>

      {/* Main Content Area */}
      <div className="max-w-6xl mx-auto w-full p-6 md:p-8 space-y-8">
        {/* 1. Three Pillars of Privacy: Data Transparency */}
        <div>
          <h2 className="text-sm font-semibold uppercase tracking-wider text-slate-400 mb-3">
            本地数据与隐私透明度
          </h2>
          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            {/* Card 1 */}
            <div className="p-5 rounded-2xl border border-slate-200 dark:border-slate-800 bg-white dark:bg-slate-900 shadow-xs flex flex-col justify-between">
              <div>
                <div className="flex items-center gap-2 text-blue-600 dark:text-blue-400 mb-2">
                  <Database className="w-5 h-5" />
                  <span className="text-sm font-bold">100% 本地优先存储</span>
                </div>
                <p className="text-xs text-slate-500 dark:text-slate-400 leading-relaxed">
                  所有浏览足迹保存在本地高可靠 SQLite WAL 中。Browsory 不架设中央用户账户服务器，更绝不收集或上传用户的任何历史记录。
                </p>
              </div>
              <div className="mt-4 pt-3 border-t border-slate-100 dark:border-slate-800 flex items-center justify-between text-[11px] text-slate-400">
                <span>存储状态: 本地独占</span>
                <span className="text-emerald-500 font-medium">无云端外溢</span>
              </div>
            </div>

            {/* Card 2 */}
            <div className="p-5 rounded-2xl border border-slate-200 dark:border-slate-800 bg-white dark:bg-slate-900 shadow-xs flex flex-col justify-between">
              <div>
                <div className="flex items-center gap-2 text-purple-600 dark:text-purple-400 mb-2">
                  <Cpu className="w-5 h-5" />
                  <span className="text-sm font-bold">AI 隐私管道脱敏</span>
                </div>
                <p className="text-xs text-slate-500 dark:text-slate-400 leading-relaxed">
                  支持连接本地 Ollama 模型（离线零泄露）；如选用云端模型，系统在发出前自动通过正则管道抹去 Token、身份、敏感 URL 参数与黑名单域名。
                </p>
              </div>
              <div className="mt-4 pt-3 border-t border-slate-100 dark:border-slate-800 flex items-center justify-between text-[11px] text-slate-400">
                <span>防泄漏脱敏: 自动拦截</span>
                <span className="text-purple-500 font-medium">本地直通保护</span>
              </div>
            </div>

            {/* Card 3 */}
            <div className="p-5 rounded-2xl border border-slate-200 dark:border-slate-800 bg-white dark:bg-slate-900 shadow-xs flex flex-col justify-between">
              <div>
                <div className="flex items-center gap-2 text-emerald-600 dark:text-emerald-400 mb-2">
                  <Cloud className="w-5 h-5" />
                  <span className="text-sm font-bold">WebDAV 隐私边界</span>
                </div>
                <p className="text-xs text-slate-500 dark:text-slate-400 leading-relaxed">
                  WebDAV 连接仅允许 HTTPS（本机回环地址除外），凭据保存在系统安全存储中。双向下载、冲突合并和落库尚未开放，应用不会虚报同步完成。
                </p>
              </div>
              <div className="mt-4 pt-3 border-t border-slate-100 dark:border-slate-800 flex items-center justify-between text-[11px] text-slate-400">
                <span>状态: 双向同步未开放</span>
                <span className="text-amber-500 font-medium">Fail-closed</span>
              </div>
            </div>
          </div>
        </div>

        {/* 2. Privacy Rules & Excluded Domains Engine */}
        <div className="p-6 rounded-2xl border border-slate-200 dark:border-slate-800 bg-white dark:bg-slate-900 shadow-xs space-y-5">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-3">
              <div className="p-2.5 rounded-xl bg-rose-50 dark:bg-rose-950/60 text-rose-600 dark:text-rose-400 border border-rose-200 dark:border-rose-900/40">
                <EyeOff className="w-5 h-5" />
              </div>
              <div>
                <h3 className="text-sm font-semibold text-slate-900 dark:text-slate-100">
                  隐私规则与黑名单管理
                </h3>
                <p className="text-xs text-slate-500 dark:text-slate-400 mt-0.5">
                  Private 规则匹配的网页将在足迹列表中隐藏；Hidden 规则匹配的网页将在 AI 与统计分析中排除
                </p>
              </div>
            </div>

            <button
              onClick={fetchRules}
              disabled={loadingRules}
              className="p-1.5 rounded-lg text-slate-400 hover:text-slate-600 dark:hover:text-slate-200 hover:bg-slate-100 dark:hover:bg-slate-800 transition cursor-pointer"
              title="刷新规则"
            >
              <RefreshCw className={`w-4 h-4 ${loadingRules ? 'animate-spin' : ''}`} />
            </button>
          </div>

          {/* Add New Rule Form */}
          <div className="p-4 rounded-xl bg-slate-50 dark:bg-slate-850/60 border border-slate-200/80 dark:border-slate-800 flex flex-col md:flex-row items-end gap-3">
            <div className="flex-1 w-full flex flex-col gap-1.5">
              <label className="text-xs font-medium text-slate-600 dark:text-slate-300">
                匹配模式 / 域名 / 路径关键字
              </label>
              <input
                type="text"
                value={newPattern}
                onChange={(e) => setNewPattern(e.target.value)}
                placeholder="例如: mail.google.com 或 bank 或 localhost"
                className="w-full px-3 py-2 text-xs rounded-xl border border-slate-200 dark:border-slate-800 bg-white dark:bg-slate-900 font-mono text-slate-800 dark:text-slate-200 focus:outline-hidden focus:ring-1 focus:ring-blue-500"
              />
            </div>

            <div className="w-full md:w-44 flex flex-col gap-1.5">
              <label className="text-xs font-medium text-slate-600 dark:text-slate-300">
                保护级别
              </label>
              <select
                value={newRuleType}
                onChange={(e) => setNewRuleType(e.target.value as 'private' | 'hidden')}
                className="w-full px-3 py-2 text-xs rounded-xl border border-slate-200 dark:border-slate-800 bg-white dark:bg-slate-900 text-slate-800 dark:text-slate-200 focus:outline-hidden focus:ring-1 focus:ring-blue-500"
              >
                <option value="private">完全私密 (Private - 隐藏足迹)</option>
                <option value="hidden">脱敏隐藏 (Hidden - 排除AI/分析)</option>
              </select>
            </div>

            <button
              onClick={handleAddRule}
              disabled={creatingRule}
              className="w-full md:w-auto px-4 py-2 rounded-xl bg-blue-600 hover:bg-blue-700 disabled:opacity-50 text-white text-xs font-medium transition flex items-center justify-center gap-1.5 cursor-pointer shadow-xs"
            >
              <Plus className="w-3.5 h-3.5" />
              <span>{creatingRule ? '添加中...' : '添加规则'}</span>
            </button>
          </div>

          {/* Rules List */}
          <div className="space-y-2">
            {rules.length === 0 ? (
              <div className="p-8 text-center text-slate-400 text-xs">
                尚未配置任何自定义隐私规则。系统仍会自动对内网 IP 与敏感鉴权 Token 进行基底防护。
              </div>
            ) : (
              rules.map((rule) => (
                <div
                  key={rule.id}
                  className="flex items-center justify-between p-3 rounded-xl border border-slate-100 dark:border-slate-800 bg-slate-50/50 dark:bg-slate-900/50 hover:bg-slate-50 dark:hover:bg-slate-850/80 transition"
                >
                  <div className="flex items-center gap-3">
                    <span className="font-mono text-xs font-medium text-slate-800 dark:text-slate-200">
                      {rule.pattern}
                    </span>
                    <span
                      className={`px-2 py-0.5 rounded text-[10px] uppercase font-mono ${
                        rule.rule_type === 'private'
                          ? 'bg-rose-100 dark:bg-rose-950/80 text-rose-700 dark:text-rose-300'
                          : 'bg-purple-100 dark:bg-purple-950/80 text-purple-700 dark:text-purple-300'
                      }`}
                    >
                      {rule.rule_type === 'private' ? '完全私密' : '排除AI/分析'}
                    </span>
                  </div>

                  <div className="flex items-center gap-3">
                    <label className="relative inline-flex items-center cursor-pointer">
                      <input
                        type="checkbox"
                        checked={rule.enabled}
                        onChange={() => handleToggleRule(rule.id, rule.enabled)}
                        className="sr-only peer"
                      />
                      <div className="w-8 h-4.5 bg-slate-200 peer-focus:outline-hidden rounded-full peer dark:bg-slate-700 peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-slate-300 after:border after:rounded-full after:h-3.5 after:w-3.5 after:transition-all dark:border-slate-600 peer-checked:bg-blue-600"></div>
                    </label>
                    <button
                      onClick={() => handleDeleteRule(rule.id)}
                      className="p-1 text-slate-400 hover:text-red-600 dark:hover:text-red-400 transition cursor-pointer"
                      title="删除规则"
                    >
                      <Trash2 className="w-4 h-4" />
                    </button>
                  </div>
                </div>
              ))
            )}
          </div>
        </div>

        {/* 3. PIN Lock & Security Settings */}
        <div className="space-y-4">
          <h2 className="text-sm font-semibold uppercase tracking-wider text-slate-400">
            应用访问锁与安全保护
          </h2>
          <SecuritySettingsCard />
        </div>

        {/* 4. Automated Backup & Maintenance */}
        <div className="space-y-4">
          <h2 className="text-sm font-semibold uppercase tracking-wider text-slate-400">
            数据备份与数据库维护
          </h2>
          <MaintenanceSettingsCard />
        </div>
      </div>
    </div>
  );
};
