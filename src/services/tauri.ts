import { invoke as rawInvoke } from '@tauri-apps/api/core';
import { openUrl as openWithSystem } from '@tauri-apps/plugin-opener';
import {
  AnalyticsSummary,
  AppInfo,
  BackupInfo,
  BatchImportResult,
  DiscoveredProfile,
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
  SyncResult,
  TagItem,
  TakeoutImportSummary,
  TestAiPayload,
  TestAiResult,
  TopicItem,
  VisitDetail,
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

function handleMock(cmd: string, _args?: any): any {
  switch (cmd) {
    case 'scan_browsers': return mockProfiles;
    case 'check_browser_running': return false;
    case 'sync_all': return [];
    case 'get_history_page': return mockHistoryPage;
    case 'get_analytics': return mockAnalytics;
    case 'get_app_info': return mockAppInfo;
    case 'get_security_state': return mockSecurityState;
    case 'get_website_ranking': return [];
    case 'get_domain_dynamics': return { new_domains: [], dormant_domains: [], rising_domains: [] };
    case 'get_interest_evolution': return [];
    case 'get_smart_collections': return [];
    case 'get_recent_import_jobs': return [];
    case 'list_privacy_rules': return [];
    case 'list_backups': return [];
    case 'list_tags': return [];
    case 'get_research_sessions': return [];
    case 'get_on_this_day': return { items: [], total_count: 0 };
    case 'get_embedding_indexing_status': return { total_urls: 100, indexed_urls: 100, is_indexing: false };
    case 'test_ai_connection': return { success: true, latency_ms: 120, message: 'AI 服务端点连接测试通过（Mock）' };
    default: return null;
  }
}

const invoke = async <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
  if (!isTauri) {
    return handleMock(cmd, args) as unknown as T;
  }
  return rawInvoke<T>(cmd, args);
};

export const tauriApi = {
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

  getSetting: async (key: string): Promise<string | null> => {
    if (!isTauri) return null;
    return invoke<string | null>('get_setting', { key });
  },

  setSetting: async (key: string, value: string): Promise<void> => {
    if (!isTauri) return;
    return invoke<void>('set_setting', { key, value });
  },

  getAnalytics: async (days?: number, startTime?: number, endTime?: number): Promise<AnalyticsSummary> => {
    if (!isTauri) return mockAnalytics;
    return invoke<AnalyticsSummary>('get_analytics', { days, startTime, endTime });
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

  callAiCompletion: async (prompt: string, systemPrompt?: string): Promise<string> => {
    if (!isTauri) return '根据您的历史浏览足迹，您近期深入查阅了 Rust、Tauri 2 以及 SQLite 相关的架构设计与文档。';
    return invoke<string>('call_ai_completion', { payload: { prompt, system_prompt: systemPrompt } });
  },

  openExternalUrl: async (url: string): Promise<void> => {
    if (!isTauri) {
      window.open(url, '_blank');
      return;
    }
    try {
      await openWithSystem(url);
    } catch (e) {
      console.error('Failed to open external url:', e);
      window.open(url, '_blank');
    }
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

  restoreDatabaseBackup: async (backupId: number): Promise<string> => {
    return invoke<string>('restore_database_backup', { backupId });
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
};


