use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub id: i64,
    pub browser: String,
    pub profile: String,
    pub source_type: String,
    pub history_path: String,
    pub db_fingerprint: Option<String>,
    pub last_visit_id: i64,
    pub last_visit_time: i64,
    pub last_sync_at: Option<i64>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisitListItem {
    pub id: i64,
    #[serde(default)]
    pub url_id: i64,
    pub visit_time: i64,
    pub title: String,
    pub url: String,
    pub domain: String,
    pub browser: String,
    pub profile: String,
    pub visit_duration: i64,
    pub transition: i64,
    #[serde(default)]
    pub is_favorite: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisitDetail {
    pub id: i64,
    pub url_id: i64,
    pub visit_time: i64,
    pub title: String,
    pub url: String,
    pub domain: String,
    pub browser: String,
    pub profile: String,
    pub source_visit_id: i64,
    pub transition: i64,
    pub from_visit: i64,
    pub visit_duration: i64,
    pub event_hash: String,
    pub imported_at: i64,
    pub history_path: String,
    #[serde(default)]
    pub is_favorite: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub total_url_visits: u64,
    #[serde(default)]
    pub first_visit_time: Option<i64>,
    #[serde(default)]
    pub last_visit_time: Option<i64>,
    #[serde(default)]
    pub nearby_visits: Vec<VisitListItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HistoryFilter {
    pub search: Option<String>,
    pub browsers: Option<Vec<String>>,
    pub profiles: Option<Vec<String>>,
    pub start_time: Option<i64>,
    pub end_time: Option<i64>,
    pub cursor_time: Option<i64>,
    pub cursor_id: Option<i64>,
    pub limit: Option<u32>,
    pub only_favorites: Option<bool>,
    pub tag: Option<String>,
    pub sort_by: Option<String>,
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagItem {
    pub id: i64,
    pub name: String,
    pub color: Option<String>,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryPageResult {
    pub items: Vec<VisitListItem>,
    pub next_cursor_time: Option<i64>,
    pub next_cursor_id: Option<i64>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    pub source_id: i64,
    pub browser: String,
    pub profile: String,
    pub read_count: u32,
    pub inserted_count: u32,
    pub duplicate_count: u32,
    pub failed_count: u32,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainStat {
    pub domain: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserStat {
    pub name: String,
    pub count: u64,
    pub percent: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HourlyStat {
    pub hour: u32,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyTrendStat {
    pub date: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQueryStat {
    pub query: String,
    pub engine: String,
    pub count: u64,
    pub last_searched_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeeklyHeatmapPoint {
    pub day_of_week: u32, // 0 = Sunday, 1 = Monday, ..., 6 = Saturday
    pub hour: u32,        // 0..23
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgottenGemItem {
    pub url_id: i64,
    pub title: String,
    pub url: String,
    pub domain: String,
    pub total_visits: u64,
    pub last_visit_time: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeriodComparison {
    pub current_count: u64,
    pub previous_count: u64,
    pub growth_rate: f64, // e.g. +15.5 for +15.5%, -8.2 for -8.2%
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchSession {
    pub session_id: String,
    pub start_time: i64,
    pub end_time: i64,
    pub duration_secs: i64,
    pub visit_count: u32,
    pub dominant_domain: String,
    pub sample_title: String,
    pub browser: String,
    pub profile: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsSummary {
    pub total_visits: u64,
    #[serde(default)]
    pub unique_urls: u64,
    #[serde(default)]
    pub unique_domains: u64,
    #[serde(default)]
    pub active_days: u64,
    #[serde(default)]
    pub revisit_rate: f64,
    pub domain_ranking: Vec<DomainStat>,
    pub browser_breakdown: Vec<BrowserStat>,
    pub hourly_distribution: Vec<HourlyStat>,
    pub daily_trend: Vec<DailyTrendStat>,
    #[serde(default)]
    pub top_search_queries: Vec<SearchQueryStat>,
    #[serde(default)]
    pub weekly_heatmap: Vec<WeeklyHeatmapPoint>,
    #[serde(default)]
    pub forgotten_gems: Vec<ForgottenGemItem>,
    pub period_comparison: PeriodComparison,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityState {
    pub pin_enabled: bool,
    pub auto_lock_minutes: i64,
    pub lock_on_minimize: bool,
    pub lock_on_sleep: bool,
    pub is_locked_out: bool,
    pub lockout_remaining_secs: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyRule {
    pub id: i64,
    pub pattern: String,
    pub rule_type: String,
    pub enabled: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupInfo {
    pub id: i64,
    pub file_name: String,
    pub file_path: String,
    pub file_size_bytes: u64,
    pub created_at: i64,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrityReport {
    pub is_healthy: bool,
    pub integrity_message: String,
    pub quick_check_message: String,
    pub total_sources: u64,
    pub total_urls: u64,
    pub total_visits: u64,
    pub fts_synced: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebMemoryCitation {
    pub visit_id: i64,
    pub title: String,
    pub url: String,
    pub domain: String,
    pub visit_time: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebMemoryAnswer {
    pub answer: String,
    pub citations: Vec<WebMemoryCitation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeSuggestion {
    pub session: ResearchSession,
    pub summary: String,
    pub next_steps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartCollection {
    pub id: i64,
    pub name: String,
    pub query: String,
    pub filter_json: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebsiteRankingItem {
    pub domain: String,
    pub visits: u64,
    pub unique_urls: u64,
    pub active_days: u64,
    pub revisit_rate: f64,
    pub first_visit_time: i64,
    pub last_visit_time: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathTreeNode {
    pub name: String,
    pub full_path: String,
    pub visits: u64,
    pub unique_urls: u64,
    pub children: Vec<PathTreeNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainTopUrl {
    pub url: String,
    pub title: String,
    pub visits: u64,
    pub last_visit_time: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainDetail {
    pub domain: String,
    pub total_visits: u64,
    pub unique_urls: u64,
    pub first_visit_time: i64,
    pub last_visit_time: i64,
    pub hourly_distribution: Vec<HourlyStat>,
    pub top_urls: Vec<DomainTopUrl>,
    pub path_tree: Vec<PathTreeNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewDomainItem {
    pub domain: String,
    pub first_visit_time: i64,
    pub sample_title: String,
    pub sample_url: String,
    pub visits_in_period: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DormantDomainItem {
    pub domain: String,
    pub total_historical_visits: u64,
    pub last_visit_time: i64,
    pub days_dormant: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainDynamics {
    pub new_domains: Vec<NewDomainItem>,
    pub dormant_domains: Vec<DormantDomainItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnThisDayItem {
    pub id: i64,
    pub url: String,
    pub title: String,
    pub domain: String,
    pub visit_time: i64,
    pub years_ago: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnThisDayResult {
    pub items: Vec<OnThisDayItem>,
    pub total_count: u32,
    pub available_years: Vec<i32>,
    pub page: u32,
    pub page_size: u32,
    pub total_pages: u32,
}

// ==========================================
// Milestone D: Semantic Memory & Hybrid Search
// ==========================================
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingIndexingStatus {
    pub total_urls: u64,
    pub indexed_urls: u64,
    pub pending_urls: u64,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridSearchResult {
    pub url_id: i64,
    pub url: String,
    pub title: String,
    pub domain: String,
    pub visit_count: u64,
    pub last_visit_time: i64,
    pub fts_rank: Option<usize>,
    pub vec_rank: Option<usize>,
    pub similarity_score: f64,
    pub rrf_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarPageItem {
    pub url_id: i64,
    pub url: String,
    pub title: String,
    pub domain: String,
    pub visit_count: u64,
    pub last_visit_time: i64,
    pub similarity: f64,
}

// ==========================================
// Milestone E: Interest Intelligence & Evolution
// ==========================================
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicItem {
    pub id: i64,
    pub name: String,
    pub category: String,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub visit_count: u64,
    pub url_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicTrend {
    pub topic_id: i64,
    pub name: String,
    pub category: String,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub current_visits: u64,
    pub previous_visits: u64,
    pub growth_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterestEvolution {
    pub rising_topics: Vec<TopicTrend>,
    pub declining_topics: Vec<TopicTrend>,
    pub new_topics: Vec<TopicTrend>,
    pub stable_topics: Vec<TopicTrend>,
}

// ==========================================
// Milestone F: Universal Archive & Link Health
// ==========================================
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirefoxProfile {
    pub profile_name: String,
    pub profile_path: String,
    pub places_db_path: String,
    pub places_size_bytes: u64,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TakeoutImportSummary {
    pub total_records_parsed: u64,
    pub urls_inserted: u64,
    pub visits_inserted: u64,
    pub duplicates_skipped: u64,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkHealthStatus {
    pub url_id: i64,
    pub url: String,
    pub status_code: Option<u16>,
    pub is_alive: bool,
    pub last_checked_at: i64,
    pub error_message: Option<String>,
    pub wayback_url: String,
}
