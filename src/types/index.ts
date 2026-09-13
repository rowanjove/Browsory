export type BrowserType = 'chrome' | 'edge' | 'brave' | 'vivaldi' | 'custom';

export interface DiscoveredProfile {
  source_id?: number | null;
  browser: string;
  profile_id: string;
  profile_name: string;
  history_path: string;
  history_size_bytes: number;
  last_modified: number | null;
  is_running: boolean;
}

export interface Source {
  id: number;
  browser: string;
  profile: string;
  source_type: string;
  history_path: string;
  db_fingerprint: string | null;
  last_visit_id: number;
  last_visit_time: number;
  last_sync_at: number | null;
  enabled: boolean;
}

export interface VisitListItem {
  id: number;
  url_id: number;
  visit_time: number;
  title: string;
  url: string;
  domain: string;
  browser: string;
  profile: string;
  visit_duration: number;
  transition: number;
  is_favorite?: boolean;
}

export interface VisitDetail {
  id: number;
  url_id: number;
  visit_time: number;
  title: string;
  url: string;
  domain: string;
  browser: string;
  profile: string;
  source_visit_id: number;
  transition: number;
  from_visit: number;
  visit_duration: number;
  event_hash: string;
  imported_at: number;
  history_path: string;
  is_favorite: boolean;
  tags: string[];
  total_url_visits: number;
  first_visit_time: number | null;
  last_visit_time: number | null;
  nearby_visits: VisitListItem[];
}

export interface HistoryFilter {
  search?: string;
  browsers?: string[];
  profiles?: string[];
  start_time?: number;
  end_time?: number;
  cursor_time?: number;
  cursor_id?: number;
  limit?: number;
  only_favorites?: boolean;
  tag?: string;
  sort_by?: 'time_desc' | 'time_asc' | 'visit_count' | 'duration';
  offset?: number;
}

export interface TagItem {
  id: number;
  name: string;
  color?: string | null;
  count: number;
}

export interface HistoryPageResult {
  items: VisitListItem[];
  next_cursor_time: number | null;
  next_cursor_id: number | null;
  has_more: boolean;
}

export interface SyncResult {
  source_id: number;
  browser: string;
  profile: string;
  read_count: number;
  inserted_count: number;
  duplicate_count: number;
  failed_count: number;
  duration_ms: number;
}

export interface AppInfo {
  app_dir: string;
  db_path: string;
  db_size_bytes: number;
  temp_dir: string;
  logs_dir: string;
}

export type NavTab = 'history' | 'analytics' | 'sources' | 'ai' | 'settings';

export interface DomainStat {
  domain: string;
  count: number;
}

export interface BrowserStat {
  name: string;
  count: number;
  percent: number;
}

export interface HourlyStat {
  hour: number;
  count: number;
}

export interface DailyTrendStat {
  date: string;
  count: number;
}

export interface SearchQueryStat {
  query: string;
  engine: string;
  count: number;
  last_searched_at: number;
}

export interface WeeklyHeatmapPoint {
  day_of_week: number; // 0 = Sunday, 1 = Monday, ..., 6 = Saturday
  hour: number;        // 0..23
  count: number;
}

export interface ForgottenGemItem {
  url_id: number;
  title: string;
  url: string;
  domain: string;
  total_visits: number;
  last_visit_time: number;
}

export interface PeriodComparison {
  current_count: number;
  previous_count: number;
  growth_rate: number;
}

export interface ResearchSession {
  session_id: string;
  start_time: number;
  end_time: number;
  duration_secs: number;
  visit_count: number;
  dominant_domain: string;
  sample_title: string;
  browser: string;
  profile: string;
}

export interface WebMemoryCitation {
  visit_id: number;
  title: string;
  url: string;
  domain: string;
  visit_time: number;
}

export interface WebMemoryAnswer {
  answer: string;
  citations: WebMemoryCitation[];
}

export interface ResumeSuggestion {
  session: ResearchSession;
  summary: string;
  next_steps: string[];
}

export interface AnalyticsSummary {
  total_visits: number;
  unique_urls: number;
  unique_domains: number;
  active_days: number;
  revisit_rate: number;
  domain_ranking: DomainStat[];
  browser_breakdown: BrowserStat[];
  hourly_distribution: HourlyStat[];
  daily_trend: DailyTrendStat[];
  top_search_queries: SearchQueryStat[];
  weekly_heatmap: WeeklyHeatmapPoint[];
  forgotten_gems: ForgottenGemItem[];
  period_comparison: PeriodComparison;
}

export interface ExportOptions {
  format: 'csv' | 'xlsx' | 'json';
  file_path: string;
  filter?: HistoryFilter;
  visit_ids?: number[];
  columns?: string[];
}

export interface TestAiPayload {
  base_url: string;
  api_key?: string;
  model: string;
}

export interface TestAiResult {
  success: boolean;
  latency_ms: number;
  message: string;
}

export interface BatchImportFileResult {
  file_path: string;
  file_name: string;
  success: boolean;
  inserted_count: number;
  duplicate_count: number;
  failed_count: number;
  error?: string;
}

export interface BatchImportResult {
  total_files: number;
  successful_files: number;
  failed_files: number;
  total_inserted: number;
  total_duplicate: number;
  results: BatchImportFileResult[];
}

export interface SecurityState {
  pin_enabled: boolean;
  auto_lock_minutes: number;
  lock_on_minimize: boolean;
  lock_on_sleep: boolean;
  is_locked_out: boolean;
  lockout_remaining_secs: number;
}

export interface PrivacyRule {
  id: number;
  pattern: string;
  rule_type: 'private' | 'hidden';
  enabled: boolean;
  created_at: number;
}

export interface BackupInfo {
  id: number;
  file_name: string;
  file_path: string;
  file_size_bytes: number;
  created_at: number;
  notes?: string | null;
}

export interface IntegrityReport {
  is_healthy: boolean;
  integrity_message: string;
  quick_check_message: string;
  total_sources: number;
  total_urls: number;
  total_visits: number;
  fts_synced: boolean;
}

export interface ImportJobItem {
  id: number;
  source_id: number | null;
  browser: string | null;
  profile: string | null;
  started_at: number;
  finished_at: number | null;
  read_count: number;
  inserted_count: number;
  duplicate_count: number;
  failed_count: number;
  status: string;
  error: string | null;
}

export interface SmartCollection {
  id: number;
  name: string;
  query: string;
  filter_json?: string | null;
  created_at: number;
}

export interface WebsiteRankingItem {
  domain: string;
  visits: number;
  unique_urls: number;
  active_days: number;
  revisit_rate: number;
  first_visit_time: number;
  last_visit_time: number;
}

export interface PathTreeNode {
  name: string;
  full_path: string;
  visits: number;
  unique_urls: number;
  children: PathTreeNode[];
}

export interface DomainTopUrl {
  url: string;
  title: string;
  visits: number;
  last_visit_time: number;
}

export interface DomainDetail {
  domain: string;
  total_visits: number;
  unique_urls: number;
  first_visit_time: number;
  last_visit_time: number;
  hourly_distribution: HourlyStat[];
  top_urls: DomainTopUrl[];
  path_tree: PathTreeNode[];
}

export interface NewDomainItem {
  domain: string;
  first_visit_time: number;
  sample_title: string;
  sample_url: string;
  visits_in_period: number;
}

export interface DormantDomainItem {
  domain: string;
  total_historical_visits: number;
  last_visit_time: number;
  days_dormant: number;
}

export interface DomainDynamics {
  new_domains: NewDomainItem[];
  dormant_domains: DormantDomainItem[];
}

export interface OnThisDayItem {
  id: number;
  url: string;
  title: string;
  domain: string;
  visit_time: number;
  years_ago: number;
}

export interface OnThisDayResult {
  items: OnThisDayItem[];
  total_count: number;
  available_years: number[];
  page: number;
  page_size: number;
  total_pages: number;
}

// Milestone D: Semantic Memory & Hybrid Search
export interface EmbeddingIndexingStatus {
  total_urls: number;
  indexed_urls: number;
  pending_urls: number;
  model: string;
}

export interface HybridSearchResult {
  url_id: number;
  url: string;
  title: string;
  domain: string;
  visit_count: number;
  last_visit_time: number;
  fts_rank?: number | null;
  vec_rank?: number | null;
  similarity_score: number;
  rrf_score: number;
}

export interface SimilarPageItem {
  url_id: number;
  url: string;
  title: string;
  domain: string;
  visit_count: number;
  last_visit_time: number;
  similarity: number;
}

// Milestone E: Interest Intelligence & Evolution
export interface TopicItem {
  id: number;
  name: string;
  category: string;
  icon?: string | null;
  color?: string | null;
  visit_count: number;
  url_count: number;
}

export interface TopicTrend {
  topic_id: number;
  name: string;
  category: string;
  icon?: string | null;
  color?: string | null;
  current_visits: number;
  previous_visits: number;
  growth_rate: number;
}

export interface InterestEvolution {
  rising_topics: TopicTrend[];
  declining_topics: TopicTrend[];
  new_topics: TopicTrend[];
  stable_topics: TopicTrend[];
}

// Milestone F: Universal Archive & Preservation
export interface FirefoxProfile {
  profile_name: string;
  profile_path: string;
  places_db_path: string;
  places_size_bytes: number;
  is_default: boolean;
}

export interface TakeoutImportSummary {
  total_records_parsed: number;
  urls_inserted: number;
  visits_inserted: number;
  duplicates_skipped: number;
  duration_ms: number;
}

export interface LinkHealthStatus {
  url_id: number;
  url: string;
  status_code?: number | null;
  is_alive: boolean;
  last_checked_at: number;
  error_message?: string | null;
  wayback_url: string;
}





