import { invoke as rawInvoke } from '@tauri-apps/api/core';
import { sensitiveRequest } from './vaultSession';
import { validateExternalHttpUrl } from './externalUrl';
import { openUrl as openWithSystem } from '@tauri-apps/plugin-opener';
import {
  AnalyticsSummary,
  AppInfo,
  AutoSyncRunItem,
  AutoSyncStatus,
  BackupInfo,
  BackgroundJob,
  BatchImportResult,
  DiagnosticsInfo,
  DiscoveredProfile,
  PageArchiveDetail,
  PageArchiveSummary,
  SaveArchivePayload,
  DomainDetail,
  DomainDynamics,
  EmbeddingIndexingStatus,
  ExportOptions,
  HistoryFilter,
  HistoryPageResult,
  HybridSearchResult,
  ImportJobItem,
  IntegrityReport,
  LinkHealthStatus,
  OnThisDayResult,
  PrivacyRule,
  ResearchSession,
  ResumeSuggestion,
  SecurityState,
  SimilarPageItem,
  SmartCollection,
  StorageBreakdown,
  StorageCleanResult,
  SyncResult,
  TagItem,
  TakeoutImportSummary,
  TestAiPayload,
  TestAiResult,
  TestEmbeddingPayload,
  TopicItem,
  PassiveResumeResult,
  VaultStatusInfo,
  VisitDetail,
  WebDavConfig,
  SyncStatusReport,
  UpdateCheckResult,
  WebsiteRankingItem,
  WebMemoryAnswer,
  InterestEvolution,
} from '../types';
import {
  mockProfiles,
  mockHistoryPage,
  mockAnalytics,
  mockAppInfo,
  mockSecurityState,
} from './mockData';

const isTauri = typeof window !== 'undefined' && Boolean((window as any).__TAURI_INTERNALS__);
const sessionControlCommands = new Set([
  'get_app_info', 'check_for_updates', 'quit_application',
  'get_vault_status', 'create_vault', 'unlock_vault', 'resume_passive_vault', 'unlock_vault_with_recovery_key',
  'lock_vault', 'change_vault_password', 'regenerate_vault_recovery_key', 'confirm_vault_recovery_key',
  'migrate_legacy_database', 'get_security_state', 'verify_pin', 'verify_recovery_key', 'reset_pin_with_recovery_key',
  'restore_database_backup', 'restore_database_backup_file',
  'retire_legacy_plaintext_backup',
]);

function handleMock(cmd: string, _args?: any): any {
  switch (cmd) {
    case 'scan_browsers': return mockProfiles;
    case 'check_browser_running': return false;
    case 'sync_all': return [];
    case 'get_history_page': return mockHistoryPage;
    case 'get_analytics': return mockAnalytics;
    case 'get_app_info': return mockAppInfo;
    case 'get_security_state': return mockSecurityState;
    case 'get_vault_status': return { status: 'unlocked', has_envelope: true, has_dpapi_wrap: true, has_plaintext_db: false, is_portable: false, recovery_key_pending: false };
    case 'resume_passive_vault': return { unlocked: true, pin_required: false, is_locked_out: false, lockout_remaining_secs: 0, error_message: null };
    case 'get_website_ranking': return [];
    case 'get_domain_dynamics': return { new_domains: [], dormant_domains: [] };
    case 'get_interest_evolution': return { rising_topics: [], declining_topics: [], new_topics: [], stable_topics: [] };
    case 'list_smart_collections':
    case 'get_smart_collections': return [];
    case 'get_recent_import_jobs': return [];
    case 'list_privacy_rules': return [];
    case 'list_backups': return [];
    case 'list_tags': return [];
    case 'get_research_sessions': return [];
    case 'get_on_this_day': return { items: [], total_count: 0 };
    case 'get_embedding_status':
    case 'get_embedding_indexing_status': return { total_urls: 100, indexed_urls: 100, pending_urls: 0, model: 'text-embedding-3-small' };
    case 'test_ai_connection': return { success: true, latency_ms: 120, message: 'AI 服务端点连接测试通过（Mock）' };
    case 'get_auto_sync_status':
    case 'repair_auto_sync_task':
      return {
        enabled: false,
        interval_days: 30,
        last_run_at: null,
        last_success_at: null,
        next_run_at: null,
        bound_sources_count: 2,
        scheduler_status: 'disabled',
        task_name: 'Browsory_AutoHistorySync',
        scheduler_error: null,
        headless_capable: true,
        is_running: false,
        is_portable: false,
        last_run: null,
        headless_notice: null,
      };
    case 'update_auto_sync_config':
      return {
        enabled: Boolean(_args?.enabled),
        interval_days: Number(_args?.intervalDays ?? 30),
        last_run_at: null,
        last_success_at: null,
        next_run_at: _args?.enabled ? Date.now() + Number(_args?.intervalDays ?? 30) * 86_400_000 : null,
        bound_sources_count: 2,
        scheduler_status: _args?.enabled ? 'registered' : 'disabled',
        task_name: 'Browsory_AutoHistorySync',
        scheduler_error: null,
        headless_capable: true,
        is_running: false,
        is_portable: false,
        last_run: null,
        headless_notice: null,
      };
    case 'run_auto_sync_now':
      return {
        id: 1,
        run_uuid: 'mock-run',
        trigger_type: 'manual',
        started_at: Date.now(),
        finished_at: Date.now(),
        status: 'success',
        sources_total: 2,
        sources_succeeded: 2,
        sources_skipped: 0,
        sources_failed: 0,
        total_read: 0,
        total_inserted: 0,
        total_duplicate: 0,
        duration_ms: 0,
        error_summary: null,
        details: [],
      };
    case 'list_auto_sync_runs': return [];
    default: return null;
  }
}

const invoke = async <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
  if (!isTauri) {
    return handleMock(cmd, args) as unknown as T;
  }
  try {
    return await (sessionControlCommands.has(cmd)
      ? rawInvoke<T>(cmd, args)
      : sensitiveRequest(() => rawInvoke<T>(cmd, args)));
  } catch (err: any) {
    const errStr = typeof err === 'string' ? err : err?.message || '';
    if (
      errStr.includes('数据库保险库未解锁') ||
      errStr.includes('VaultLocked') ||
      errStr.toLowerCase().includes('vault locked')
    ) {
      if (typeof window !== 'undefined') {
        window.dispatchEvent(new CustomEvent('browsory://vault-locked'));
      }
    }
    throw err;
  }
};

export const tauriApi = {
  getPlaintextResidue: (): Promise<{ legacy_backup_bytes: number | null; blocked: boolean }> =>
    invoke('get_plaintext_residue'),
  retireLegacyPlaintextBackup: (password: string, confirmation: string): Promise<string> =>
    invoke('retire_legacy_plaintext_backup', { password, confirmation }),
  restoreDatabaseBackupFile: (filePath: string, password?: string, recoveryKey?: string): Promise<string> =>
    invoke<string>('restore_database_backup_file', { filePath, password, recoveryKey }),
  scanBrowsers: async (): Promise<DiscoveredProfile[]> => {
    if (!isTauri) return mockProfiles;
    return invoke<DiscoveredProfile[]>('scan_browsers');
  },

  checkBrowserRunning: async (browser: string): Promise<boolean> => {
    if (!isTauri) return false;
    return invoke<boolean>('check_browser_running', { browser });
  },

  killBrowser: async (browser: string, force: boolean): Promise<void> => {
    return invoke<void>('kill_browser', { browser, force });
  },

  syncSource: async (sourceId: number): Promise<SyncResult> => {
    return invoke<SyncResult>('sync_source', { sourceId });
  },

  syncAll: async (): Promise<SyncResult[]> => {
    if (!isTauri) return [];
    return invoke<SyncResult[]>('sync_all');
  },

  getHistoryPage: async (filter: HistoryFilter): Promise<HistoryPageResult> => {
    if (!isTauri) return mockHistoryPage;
    return invoke<HistoryPageResult>('get_history_page', { filter });
  },

  getVisitDetail: async (visitId: number): Promise<VisitDetail | null> => {
    if (!isTauri) return null;
    return invoke<VisitDetail | null>('get_visit_detail', { visitId });
  },

  deleteVisits: async (visitIds: number[]): Promise<number> => {
    if (!isTauri) return 0;
    return invoke<number>('delete_visits', { visitIds });
  },

  getAppInfo: async (): Promise<AppInfo> => {
    if (!isTauri) return mockAppInfo;
    return invoke<AppInfo>('get_app_info');
  },

  checkForUpdates: async (): Promise<UpdateCheckResult> => {
    return invoke<UpdateCheckResult>('check_for_updates');
  },

  quitApplication: async (): Promise<void> => {
    return invoke<void>('quit_application');
  },

  getSetting: async (key: string): Promise<string | null> => {
    if (!isTauri) return null;
    return invoke<string | null>('get_setting', { key });
  },

  setSetting: async (key: string, value: string): Promise<void> => {
    if (!isTauri) return;
    return invoke<void>('set_setting', { key, value });
  },

  getAnalytics: async (
    daysOrOptions?: number | { days?: number; startTime?: number; endTime?: number; summaryOnly?: boolean },
    startTime?: number,
    endTime?: number,
    summaryOnly?: boolean
  ): Promise<AnalyticsSummary> => {
    if (!isTauri) return mockAnalytics;
    let payload: { days?: number; startTime?: number; endTime?: number; summaryOnly?: boolean } = {};
    if (typeof daysOrOptions === 'object' && daysOrOptions !== null) {
      payload = daysOrOptions;
    } else if (typeof daysOrOptions === 'number' && daysOrOptions > 3650) {
      // If a large number (timestamp in ms) was passed as first argument, map to startTime
      payload = { startTime: daysOrOptions, endTime: startTime, summaryOnly };
    } else {
      payload = { days: daysOrOptions, startTime, endTime, summaryOnly };
    }
    return invoke<AnalyticsSummary>('get_analytics', payload);
  },

  exportHistory: async (options: ExportOptions): Promise<number> => {
    if (!isTauri) return 0;
    return invoke<number>('export_history', { options });
  },

  toggleSourceEnabled: async (sourceId: number, enabled: boolean): Promise<void> => {
    if (!isTauri) return;
    return invoke<void>('toggle_source_enabled', { sourceId, enabled });
  },

  openPathInFolder: async (path: string): Promise<void> => {
    if (!isTauri) {
      console.log('Open path in folder (mock):', path);
      return;
    }
    return invoke<void>('open_path_in_folder', { path });
  },

  importHistoryFile: async (filePath: string, browserName?: string, profileName?: string): Promise<SyncResult> => {
    return invoke<SyncResult>('import_history_file', { filePath, browserName, profileName });
  },

  importHistoryBatch: async (filePaths: string[]): Promise<BatchImportResult> => {
    return invoke<BatchImportResult>('import_history_batch', { filePaths });
  },

  testAiConnection: async (payload: TestAiPayload): Promise<TestAiResult> => {
    if (!isTauri) return { success: true, latency_ms: 120, message: 'AI 服务端点连接测试通过（Mock）' };
    return invoke<TestAiResult>('test_ai_connection', { payload });
  },

  testEmbeddingConnection: async (payload: TestEmbeddingPayload): Promise<TestAiResult> => {
    if (!isTauri) return { success: true, latency_ms: 95, message: '向量端点测试通过 (Mock 维度: 1536)' };
    return invoke<TestAiResult>('test_embedding_connection', { payload });
  },

  callAiCompletion: async (prompt: string, systemPrompt?: string): Promise<string> => {
    if (!isTauri) return '根据您的历史浏览足迹，您近期深入查阅了 Rust、Tauri 2 以及 SQLite 相关的架构设计与文档。';
    return invoke<string>('call_ai_completion', { payload: { prompt, system_prompt: systemPrompt } });
  },

  openExternalUrl: async (url: string): Promise<void> => {
    const safeUrl = validateExternalHttpUrl(url);
    if (!isTauri) {
      window.open(safeUrl, '_blank', 'noopener,noreferrer');
      return;
    }
    try {
      await openWithSystem(safeUrl);
    } catch (e) {
      console.error('Failed to open external url:', e);
      window.open(safeUrl, '_blank', 'noopener,noreferrer');
    }
  },

  // Database Security Vault
  regenerateVaultRecoveryKey: (password: string): Promise<string> => invoke<string>('regenerate_vault_recovery_key', { password }),
  regeneratePassiveVaultRecoveryKey: (): Promise<string> => invoke<string>('regenerate_passive_vault_recovery_key'),
  confirmVaultRecoveryKey: (recoveryKey: string): Promise<void> => invoke<void>('confirm_vault_recovery_key', { recoveryKey }),
  getVaultStatus: async (): Promise<VaultStatusInfo> => {
    return invoke<VaultStatusInfo>('get_vault_status');
  },

  createVault: async (password: string): Promise<string> => {
    return invoke<string>('create_vault', { password });
  },

  unlockVault: async (password: string): Promise<boolean> => {
    return invoke<boolean>('unlock_vault', { password });
  },

  resumePassiveVault: async (pin?: string): Promise<PassiveResumeResult> => {
    return invoke<PassiveResumeResult>('resume_passive_vault', pin ? { pin } : {});
  },

  unlockVaultWithRecoveryKey: async (recoveryKey: string): Promise<boolean> => {
    return invoke<boolean>('unlock_vault_with_recovery_key', { recoveryKey });
  },

  lockVault: async (): Promise<void> => {
    return invoke<void>('lock_vault');
  },

  changeVaultPassword: async (oldPassword: string, newPassword: string): Promise<void> => {
    return invoke<void>('change_vault_password', { oldPassword, newPassword });
  },

  migrateLegacyDatabase: async (password: string): Promise<string> => {
    return invoke<string>('migrate_legacy_database', { password });
  },

  // Security & App Lock
  getSecurityState: async (): Promise<SecurityState> => {
    if (!isTauri) return mockSecurityState;
    return invoke<SecurityState>('get_security_state');
  },

  verifyPin: async (pin: string): Promise<boolean> => {
    return invoke<boolean>('verify_pin', { pin });
  },

  setOrChangePin: async (oldPin: string | null, newPin: string): Promise<string | null> => {
    return invoke<string | null>('set_or_change_pin', { oldPin, newPin });
  },

  verifyRecoveryKey: async (recoveryKey: string): Promise<boolean> => {
    return invoke<boolean>('verify_recovery_key', { recoveryKey });
  },

  resetPinWithRecoveryKey: async (recoveryKey: string, newPin: string): Promise<void> => {
    return invoke<void>('reset_pin_with_recovery_key', { recoveryKey, newPin });
  },

  disablePin: async (currentPin: string): Promise<void> => {
    return invoke<void>('disable_pin', { currentPin });
  },

  updateSecurityOptions: async (autoLockMinutes: number, lockOnMinimize: boolean, lockOnSleep: boolean): Promise<void> => {
    return invoke<void>('update_security_options', { autoLockMinutes, lockOnMinimize, lockOnSleep });
  },

  // Privacy Rules
  listPrivacyRules: async (): Promise<PrivacyRule[]> => {
    return invoke<PrivacyRule[]>('list_privacy_rules');
  },

  addPrivacyRule: async (pattern: string, ruleType: 'private' | 'hidden'): Promise<PrivacyRule> => {
    return invoke<PrivacyRule>('add_privacy_rule', { pattern, ruleType });
  },

  togglePrivacyRule: async (id: number, enabled: boolean): Promise<void> => {
    return invoke<void>('toggle_privacy_rule', { id, enabled });
  },

  deletePrivacyRule: async (id: number): Promise<void> => {
    return invoke<void>('delete_privacy_rule', { id });
  },

  // Backups & Maintenance
  createDatabaseBackup: async (notes?: string): Promise<BackupInfo> => {
    return invoke<BackupInfo>('create_database_backup', { notes });
  },

  listBackups: async (): Promise<BackupInfo[]> => {
    return invoke<BackupInfo[]>('list_backups');
  },

  deleteBackup: async (id: number): Promise<void> => {
    return invoke<void>('delete_backup', { id });
  },

  restoreDatabaseBackup: async (backupId: number, password?: string, recoveryKey?: string): Promise<string> => {
    return invoke<string>('restore_database_backup', { backupId, password, recoveryKey });
  },

  exportDatabaseBackup: async (backupId: number, targetFilePath: string): Promise<void> => {
    return invoke<void>('export_database_backup', { backupId, targetFilePath });
  },

  checkDatabaseIntegrity: async (): Promise<IntegrityReport> => {
    return invoke<IntegrityReport>('check_database_integrity');
  },

  vacuumDatabase: async (): Promise<void> => {
    return invoke<void>('vacuum_database');
  },

  // Sync Center
  getRecentImportJobs: async (limit?: number): Promise<ImportJobItem[]> => {
    return invoke<ImportJobItem[]>('get_recent_import_jobs', { limit });
  },

  // Favorites & Tags
  toggleFavorite: async (urlId: number): Promise<boolean> => {
    return invoke<boolean>('toggle_favorite', { urlId });
  },

  listTags: async (): Promise<TagItem[]> => {
    return invoke<TagItem[]>('list_tags');
  },

  addTagToUrl: async (urlId: number, tagName: string): Promise<void> => {
    return invoke<void>('add_tag_to_url', { urlId, tagName });
  },

  removeTagFromUrl: async (urlId: number, tagName: string): Promise<void> => {
    return invoke<void>('remove_tag_from_url', { urlId, tagName });
  },

  // Research Sessions
  getResearchSessions: async (days?: number, limit?: number): Promise<ResearchSession[]> => {
    return invoke<ResearchSession[]>('get_research_sessions', { days, limit });
  },

  // AI Web Memory & Resume
  askWebMemory: async (question: string): Promise<WebMemoryAnswer> => {
    return invoke<WebMemoryAnswer>('ask_web_memory', { question });
  },

  getResumeSuggestion: async (): Promise<ResumeSuggestion> => {
    return invoke<ResumeSuggestion>('get_resume_suggestion');
  },

  // Milestone C: Website Ranking, Domain Detail, Dynamics, On This Day & Smart Collections
  getWebsiteRanking: async (startTime?: number, endTime?: number, limit?: number): Promise<WebsiteRankingItem[]> => {
    return invoke<WebsiteRankingItem[]>('get_website_ranking', { startTime, endTime, limit });
  },

  getDomainDetail: async (domain: string, startTime?: number, endTime?: number): Promise<DomainDetail> => {
    return invoke<DomainDetail>('get_domain_detail', { domain, startTime, endTime });
  },

  getDomainDynamics: async (startTime?: number, endTime?: number): Promise<DomainDynamics> => {
    return invoke<DomainDynamics>('get_domain_dynamics', { startTime, endTime });
  },

  getOnThisDay: async (targetDate?: string, year?: number, page?: number, pageSize?: number): Promise<OnThisDayResult> => {
    return invoke<OnThisDayResult>('get_on_this_day', { targetDate, year, page, pageSize });
  },

  listSmartCollections: async (): Promise<SmartCollection[]> => {
    return invoke<SmartCollection[]>('list_smart_collections');
  },

  createSmartCollection: async (name: string, query: string, filterJson?: string): Promise<SmartCollection> => {
    return invoke<SmartCollection>('create_smart_collection', { name, query, filterJson });
  },

  deleteSmartCollection: async (id: number): Promise<void> => {
    return invoke<void>('delete_smart_collection', { id });
  },

  // Milestone D: Semantic Memory & Hybrid Search
  getEmbeddingStatus: async (model?: string): Promise<EmbeddingIndexingStatus> => {
    return invoke<EmbeddingIndexingStatus>('get_embedding_status', { model });
  },

  generateEmbeddingsBatch: async (model?: string, batchSize?: number): Promise<number> => {
    return invoke<number>('generate_embeddings_batch', { model, batchSize });
  },

  hybridSearch: async (query: string, limit?: number): Promise<HybridSearchResult[]> => {
    return invoke<HybridSearchResult[]>('hybrid_search', { query, limit });
  },

  getSimilarPages: async (urlId: number, model?: string, limit?: number): Promise<SimilarPageItem[]> => {
    return invoke<SimilarPageItem[]>('get_similar_pages', { urlId, model, limit });
  },

  // Milestone E: Interest Intelligence & Evolution
  getInterestEvolution: async (days?: number): Promise<InterestEvolution> => {
    return invoke<InterestEvolution>('get_interest_evolution', { days });
  },

  getAllTopics: async (): Promise<TopicItem[]> => {
    return invoke<TopicItem[]>('get_all_topics');
  },

  generateAiPeriodComparison: async (days?: number): Promise<string> => {
    return invoke<string>('generate_ai_period_comparison', { days });
  },

  // Milestone F: Universal Archive & Preservation
  importTakeoutFile: async (filePath: string): Promise<TakeoutImportSummary> => {
    return invoke<TakeoutImportSummary>('import_takeout_file', { filePath });
  },

  checkLinkHealth: async (urlId: number): Promise<LinkHealthStatus> => {
    return invoke<LinkHealthStatus>('check_link_health', { urlId });
  },

  getLinkHealth: async (urlId: number): Promise<LinkHealthStatus | null> => {
    return invoke<LinkHealthStatus | null>('get_link_health', { urlId });
  },

  // Milestone D: Diagnostics & Health Center
  getDiagnosticsInfo: async (): Promise<DiagnosticsInfo> => {
    return invoke<DiagnosticsInfo>('get_diagnostics_info');
  },

  exportDiagnosticsBundle: async (targetPath?: string): Promise<string> => {
    return invoke<string>('export_diagnostics_bundle', { targetPath });
  },

  // Milestone E: Storage Manager
  getStorageBreakdown: async (): Promise<StorageBreakdown> => {
    return invoke<StorageBreakdown>('get_storage_breakdown');
  },

  cleanStorageCache: async (): Promise<StorageCleanResult> => {
    return invoke<StorageCleanResult>('clean_storage_cache');
  },

  // Milestone G: Offline Page Archives & Background Jobs
  saveOfflineArchive: async (payload: SaveArchivePayload): Promise<PageArchiveSummary> => {
    return invoke<PageArchiveSummary>('save_offline_archive', { payload });
  },

  getOfflineArchive: async (urlId: number): Promise<PageArchiveDetail | null> => {
    return invoke<PageArchiveDetail | null>('get_offline_archive', { urlId });
  },

  listOfflineArchives: async (limit?: number, offset?: number): Promise<PageArchiveSummary[]> => {
    return invoke<PageArchiveSummary[]>('list_offline_archives', { limit, offset });
  },

  deleteOfflineArchive: async (pageUuid: string): Promise<void> => {
    return invoke<void>('delete_offline_archive', { pageUuid });
  },

  listBackgroundJobs: async (limit?: number): Promise<BackgroundJob[]> => {
    return invoke<BackgroundJob[]>('list_background_jobs', { limit });
  },

  cancelBackgroundJob: async (jobId: string): Promise<void> => {
    return invoke<void>('cancel_background_job', { jobId });
  },

  // Milestone H: E2EE Multi-Device Sync
  testWebDavSync: async (config: WebDavConfig): Promise<void> => {
    return invoke<void>('test_webdav_sync', { config });
  },

  executeWebDavSync: async (config: WebDavConfig): Promise<SyncStatusReport> => {
    return invoke<SyncStatusReport>('execute_webdav_sync', { config });
  },

  // Auto History Sync (Scheduled Incremental)
  getAutoSyncStatus: async (): Promise<AutoSyncStatus> => {
    return invoke<AutoSyncStatus>('get_auto_sync_status');
  },

  updateAutoSyncConfig: async (enabled: boolean, intervalDays: number): Promise<AutoSyncStatus> => {
    return invoke<AutoSyncStatus>('update_auto_sync_config', { enabled, intervalDays });
  },

  runAutoSyncNow: async (): Promise<AutoSyncRunItem> => {
    return invoke<AutoSyncRunItem>('run_auto_sync_now');
  },

  repairAutoSyncTask: async (): Promise<AutoSyncStatus> => {
    return invoke<AutoSyncStatus>('repair_auto_sync_task');
  },

  listAutoSyncRuns: async (limit?: number): Promise<AutoSyncRunItem[]> => {
    return invoke<AutoSyncRunItem[]>('list_auto_sync_runs', { limit });
  },
};


