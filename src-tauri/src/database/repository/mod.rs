use rusqlite::{params, Connection, OptionalExtension};
use tracing::info;

#[cfg(test)]
use super::models::HistoryFilter;
use super::models::{
    AnalyticsSummary, BackupInfo, BrowserStat, DailyTrendStat, DomainDetail, DomainDynamics,
    DomainStat, DomainTopUrl, DormantDomainItem, EmbeddingIndexingStatus, ForgottenGemItem,
    HourlyStat, HybridSearchResult, IntegrityReport, InterestEvolution, NewDomainItem,
    OnThisDayItem, OnThisDayResult, PathTreeNode, PeriodComparison, PrivacyRule, ResearchSession,
    SearchQueryStat, SecurityState, SimilarPageItem, SmartCollection, TagItem, TopicItem,
    TopicTrend, VisitDetail, VisitListItem, WebMemoryCitation, WebsiteRankingItem,
    WeeklyHeatmapPoint,
};
use crate::error::{AppError, AppResult};

mod privacy_policy;
use privacy_policy::escape_like_literal;
pub use privacy_policy::{build_hidden_rule_conditions, matches_privacy_rule};
use privacy_policy::{build_rule_conditions, DataScope};

pub mod auto_sync;
pub use auto_sync::*;

pub mod sources;
pub use sources::*;

pub(crate) mod history_query;
pub use history_query::query_history;

pub fn get_visit_detail(conn: &Connection, visit_id: i64) -> AppResult<Option<VisitDetail>> {
    // Hidden rules are an output boundary, so load them before querying any
    // details and fail closed if the rule table cannot be read.
    let active_rules = get_active_privacy_rules(conn)?;
    let hidden_rules: Vec<_> = active_rules
        .iter()
        .filter(|r| r.rule_type == "hidden")
        .collect();

    let base_info = conn
        .query_row(
            r#"
            SELECT v.id, v.url_id, v.visit_time, COALESCE(u.title, ''), u.url, COALESCE(u.domain, ''),
                   s.browser, s.profile, v.source_visit_id, v.transition, v.from_visit,
                   v.visit_duration, v.event_hash, v.imported_at, s.history_path,
                   EXISTS(SELECT 1 FROM favorites f WHERE f.url_id = u.id) as is_favorite
            FROM visits v
            JOIN urls u ON v.url_id = u.id
            JOIN sources s ON v.source_id = s.id
            WHERE v.id = ?1
            "#,
            params![visit_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, i64>(10)?,
                    row.get::<_, i64>(11)?,
                    row.get::<_, String>(12)?,
                    row.get::<_, i64>(13)?,
                    row.get::<_, String>(14)?,
                    row.get::<_, i64>(15)? == 1,
                ))
            },
        )
        .optional()?;

    let (
        id,
        url_id,
        visit_time,
        title,
        url,
        domain,
        browser,
        profile,
        source_visit_id,
        transition,
        from_visit,
        visit_duration,
        event_hash,
        imported_at,
        history_path,
        is_favorite,
    ) = match base_info {
        Some(b) => b,
        None => return Ok(None),
    };

    // A detail request can target an id that was obtained before a rule was
    // enabled. Never return the current record when it is now hidden.
    if hidden_rules
        .iter()
        .any(|rule| matches_privacy_rule(&rule.pattern, &domain, &url))
    {
        return Ok(None);
    }

    let mut hidden_conds = Vec::new();
    let mut hidden_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    build_hidden_rule_conditions(
        &active_rules,
        "u.domain",
        "u.url",
        &mut hidden_conds,
        &mut hidden_params,
    );
    let hidden_clause = if hidden_conds.is_empty() {
        String::new()
    } else {
        format!("AND {}", hidden_conds.join(" AND "))
    };

    // Query Tags for this URL
    let mut tags = Vec::new();
    if let Ok(mut tag_stmt) = conn.prepare(
        "SELECT t.name FROM tags t JOIN url_tags ut ON ut.tag_id = t.id WHERE ut.url_id = ?1 ORDER BY t.name ASC"
    ) {
        if let Ok(tag_rows) = tag_stmt.query_map([url_id], |r| r.get::<_, String>(0)) {
            for t in tag_rows.flatten() {
                tags.push(t);
            }
        }
    }

    // Query URL visit statistics
    let (total_url_visits, first_visit_time, last_visit_time) = conn
        .query_row(
            "SELECT count(*), min(visit_time), max(visit_time) FROM visits WHERE url_id = ?1",
            params![url_id],
            |r| {
                Ok((
                    r.get::<_, i64>(0).unwrap_or(1) as u64,
                    r.get::<_, Option<i64>>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                ))
            },
        )
        .unwrap_or((1, Some(visit_time), Some(visit_time)));

    // Query Nearby Visits (4 before and 4 after in time)
    let mut nearby_visits = Vec::new();

    // 4 before
    let before_sql = format!(
        r#"
        SELECT v.id, v.url_id, v.visit_time, COALESCE(u.title, '') as title, u.url, COALESCE(u.domain, '') as domain,
               s.browser, s.profile, v.visit_duration, v.transition,
               EXISTS(SELECT 1 FROM favorites f WHERE f.url_id = u.id) as is_favorite
        FROM visits v
        JOIN urls u ON v.url_id = u.id
        JOIN sources s ON v.source_id = s.id
        WHERE v.visit_time < ? {}
        ORDER BY v.visit_time DESC, v.id DESC
        LIMIT 4
        "#,
        hidden_clause
    );
    let mut before_stmt = conn.prepare(&before_sql)?;
    let mut before_params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(1 + hidden_params.len());
    before_params.push(&visit_time);
    for p in &hidden_params {
        before_params.push(p.as_ref());
    }
    let before_rows = before_stmt.query_map(before_params.as_slice(), |row| {
        Ok(VisitListItem {
            id: row.get(0)?,
            url_id: row.get(1)?,
            visit_time: row.get(2)?,
            title: row.get(3)?,
            url: row.get(4)?,
            domain: row.get(5)?,
            browser: row.get(6)?,
            profile: row.get(7)?,
            visit_duration: row.get(8)?,
            transition: row.get(9)?,
            is_favorite: row.get::<_, i64>(10)? == 1,
        })
    })?;
    let mut before_items = Vec::new();
    for r in before_rows.flatten() {
        before_items.push(r);
    }
    before_items.reverse(); // Chronological order
    nearby_visits.extend(before_items);

    // Current item
    nearby_visits.push(VisitListItem {
        id,
        url_id,
        visit_time,
        title: title.clone(),
        url: url.clone(),
        domain: domain.clone(),
        browser: browser.clone(),
        profile: profile.clone(),
        visit_duration,
        transition,
        is_favorite,
    });

    // 4 after
    let after_sql = format!(
        r#"
        SELECT v.id, v.url_id, v.visit_time, COALESCE(u.title, '') as title, u.url, COALESCE(u.domain, '') as domain,
               s.browser, s.profile, v.visit_duration, v.transition,
               EXISTS(SELECT 1 FROM favorites f WHERE f.url_id = u.id) as is_favorite
        FROM visits v
        JOIN urls u ON v.url_id = u.id
        JOIN sources s ON v.source_id = s.id
        WHERE v.visit_time > ? {}
        ORDER BY v.visit_time ASC, v.id ASC
        LIMIT 4
        "#,
        hidden_clause
    );
    let mut after_stmt = conn.prepare(&after_sql)?;
    let mut after_params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(1 + hidden_params.len());
    after_params.push(&visit_time);
    for p in &hidden_params {
        after_params.push(p.as_ref());
    }
    let after_rows = after_stmt.query_map(after_params.as_slice(), |row| {
        Ok(VisitListItem {
            id: row.get(0)?,
            url_id: row.get(1)?,
            visit_time: row.get(2)?,
            title: row.get(3)?,
            url: row.get(4)?,
            domain: row.get(5)?,
            browser: row.get(6)?,
            profile: row.get(7)?,
            visit_duration: row.get(8)?,
            transition: row.get(9)?,
            is_favorite: row.get::<_, i64>(10)? == 1,
        })
    })?;
    for r in after_rows.flatten() {
        nearby_visits.push(r);
    }

    Ok(Some(VisitDetail {
        id,
        url_id,
        visit_time,
        title,
        url,
        domain,
        browser,
        profile,
        source_visit_id,
        transition,
        from_visit,
        visit_duration,
        event_hash,
        imported_at,
        history_path,
        is_favorite,
        tags,
        total_url_visits,
        first_visit_time,
        last_visit_time,
        nearby_visits,
    }))
}

// ==========================================
// Favorites & Tags Repository Methods
// ==========================================

pub fn toggle_favorite(conn: &Connection, url_id: i64) -> AppResult<bool> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM favorites WHERE url_id = ?1)",
        params![url_id],
        |r| r.get(0),
    )?;

    if exists {
        conn.execute("DELETE FROM favorites WHERE url_id = ?1", params![url_id])?;
        Ok(false)
    } else {
        let now = chrono::Utc::now().timestamp_millis();
        conn.execute(
            "INSERT INTO favorites (url_id, created_at) VALUES (?1, ?2)",
            params![url_id, now],
        )?;
        Ok(true)
    }
}

pub fn list_tags(conn: &Connection) -> AppResult<Vec<TagItem>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT t.id, t.name, t.color, count(ut.url_id)
        FROM tags t
        LEFT JOIN url_tags ut ON ut.tag_id = t.id
        GROUP BY t.id
        ORDER BY count(ut.url_id) DESC, t.name ASC
        "#,
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(TagItem {
            id: row.get(0)?,
            name: row.get(1)?,
            color: row.get(2)?,
            count: row.get::<_, i64>(3)? as u64,
        })
    })?;

    let mut list = Vec::new();
    for r in rows {
        list.push(r?);
    }
    Ok(list)
}

pub fn add_tag_to_url(conn: &Connection, url_id: i64, tag_name: &str) -> AppResult<()> {
    let trimmed = tag_name.trim();
    if trimmed.is_empty() {
        return Ok(());
    }

    let now = chrono::Utc::now().timestamp_millis();
    conn.execute(
        "INSERT OR IGNORE INTO tags (name, created_at) VALUES (?1, ?2)",
        params![trimmed, now],
    )?;

    let tag_id: i64 = conn.query_row(
        "SELECT id FROM tags WHERE name = ?1",
        params![trimmed],
        |r| r.get(0),
    )?;

    conn.execute(
        "INSERT OR IGNORE INTO url_tags (url_id, tag_id, created_at) VALUES (?1, ?2, ?3)",
        params![url_id, tag_id, now],
    )?;

    Ok(())
}

pub fn remove_tag_from_url(conn: &Connection, url_id: i64, tag_name: &str) -> AppResult<()> {
    conn.execute(
        r#"
        DELETE FROM url_tags
        WHERE url_id = ?1 AND tag_id = (SELECT id FROM tags WHERE name = ?2)
        "#,
        params![url_id, tag_name.trim()],
    )?;
    Ok(())
}

pub fn delete_visits(conn: &Connection, visit_ids: &[i64]) -> AppResult<u32> {
    delete_visits_checked(conn, visit_ids, || Ok(()))
}

pub fn delete_visits_checked(
    conn: &Connection,
    visit_ids: &[i64],
    check: impl Fn() -> AppResult<()>,
) -> AppResult<u32> {
    check()?;
    if visit_ids.is_empty() {
        return Ok(0);
    }

    // One user action is atomic even when split to stay below SQLite's
    // parameter limit. A later statement error or SQL cancellation must undo
    // the earlier chunks too.
    let transaction = conn.unchecked_transaction()?;
    let mut total_deleted = 0;
    let deleted_at = chrono::Utc::now().timestamp_millis();
    // Chunk by 500 to avoid SQLite SQLITE_MAX_VARIABLE_NUMBER limits
    for chunk in visit_ids.chunks(500) {
        check()?;
        let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let numbered_placeholders = chunk
            .iter()
            .enumerate()
            .map(|(index, _)| format!("?{}", index + 2))
            .collect::<Vec<_>>()
            .join(",");
        let tombstone_sql = format!(
            "INSERT OR IGNORE INTO visit_exclusions(event_hash,source_id,source_visit_id,deleted_at) \
             SELECT event_hash,source_id,source_visit_id,?1 FROM visits WHERE id IN ({})",
            numbered_placeholders
        );
        let mut tombstone_params: Vec<&dyn rusqlite::ToSql> = vec![&deleted_at];
        tombstone_params.extend(chunk.iter().map(|id| id as &dyn rusqlite::ToSql));
        transaction.execute(&tombstone_sql, tombstone_params.as_slice())?;

        let sql = format!("DELETE FROM visits WHERE id IN ({})", placeholders);

        let params: Vec<&dyn rusqlite::ToSql> =
            chunk.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
        let deleted = transaction.execute(&sql, params.as_slice())?;
        total_deleted += deleted;
    }

    check()?;
    transaction.commit()?;
    info!("Deleted {} visits from archive.db", total_deleted);
    Ok(total_deleted as u32)
}

pub fn get_setting(conn: &Connection, key: &str) -> AppResult<Option<String>> {
    let res = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()?;
    Ok(res)
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> AppResult<()> {
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
        params![key, value],
    )?;
    Ok(())
}

pub fn toggle_source_enabled(conn: &Connection, source_id: i64, enabled: bool) -> AppResult<()> {
    conn.execute(
        "UPDATE sources SET enabled = ?1 WHERE id = ?2",
        params![if enabled { 1 } else { 0 }, source_id],
    )?;
    Ok(())
}

fn empty_analytics_summary(
    total_visits: u64,
    unique_urls: u64,
    unique_domains: u64,
    revisit_rate: f64,
) -> AnalyticsSummary {
    AnalyticsSummary {
        total_visits,
        unique_urls,
        unique_domains,
        active_days: 0,
        revisit_rate,
        domain_ranking: Vec::new(),
        browser_breakdown: Vec::new(),
        hourly_distribution: (0..24).map(|hour| HourlyStat { hour, count: 0 }).collect(),
        daily_trend: Vec::new(),
        top_search_queries: Vec::new(),
        weekly_heatmap: Vec::new(),
        forgotten_gems: Vec::new(),
        period_comparison: PeriodComparison {
            current_count: total_visits,
            previous_count: 0,
            growth_rate: 0.0,
        },
    }
}

pub fn get_analytics_summary(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    days: Option<u32>,
) -> AppResult<AnalyticsSummary> {
    get_analytics_summary_with_options(conn, start_time, end_time, days, false)
}

pub fn get_analytics_summary_with_options(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    days: Option<u32>,
    summary_only: bool,
) -> AppResult<AnalyticsSummary> {
    use chrono::{Datelike, Local, TimeZone, Timelike};

    let now_ms = chrono::Utc::now().timestamp_millis();
    let (min_visit_time, max_visit_time) = match (start_time, end_time) {
        (Some(st), Some(et)) => (st, et),
        (Some(st), None) => (st, now_ms),
        (None, Some(et)) => (0, et),
        (None, None) => match days {
            Some(d) if d > 0 => (now_ms - (d as i64) * 86_400_000, now_ms),
            _ => (0, now_ms),
        },
    };

    let active_rules = get_active_privacy_rules(conn)?;
    let mut hidden_conds = Vec::new();
    let mut hidden_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    build_hidden_rule_conditions(
        &active_rules,
        "u.domain",
        "u.url",
        &mut hidden_conds,
        &mut hidden_params,
    );
    let has_hidden = !hidden_conds.is_empty();
    let hidden_clause = if !has_hidden {
        String::new()
    } else {
        format!("AND {}", hidden_conds.join(" AND "))
    };

    let mut time_params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(2 + hidden_params.len());
    time_params.push(&min_visit_time);
    time_params.push(&max_visit_time);
    for b in &hidden_params {
        time_params.push(b.as_ref());
    }

    // Count the same visible time range once. Empty aggregates return zero;
    // query failures propagate instead of masquerading as empty history.
    let (total_visits, unique_urls, unique_domains): (u64, u64, u64) = conn.query_row(
        &format!(
            r#"SELECT count(*), count(DISTINCT u.id),
                      count(DISTINCT CASE WHEN u.domain != '' THEN u.domain END)
               FROM visits v JOIN urls u ON v.url_id = u.id
               WHERE v.visit_time >= ? AND v.visit_time <= ? {}"#,
            hidden_clause
        ),
        time_params.as_slice(),
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;

    // 1e. Revisit Rate
    let revisit_rate = if total_visits > 0 {
        ((total_visits as f64 - unique_urls as f64) / total_visits as f64 * 1000.0).round() / 10.0
    } else {
        0.0
    }
    .max(0.0);

    if summary_only {
        return Ok(empty_analytics_summary(
            total_visits,
            unique_urls,
            unique_domains,
            revisit_rate,
        ));
    }

    // 2. Domain ranking (Top 10)
    let domain_sql = format!(
        r#"
        SELECT u.domain, count(v.id) as cnt
        FROM visits v
        JOIN urls u ON v.url_id = u.id
        WHERE v.visit_time >= ? AND v.visit_time <= ? AND u.domain IS NOT NULL AND u.domain != '' {}
        GROUP BY u.domain
        ORDER BY cnt DESC
        LIMIT 10
        "#,
        hidden_clause
    );
    let mut domain_stmt = conn.prepare(&domain_sql)?;
    let domain_rows = domain_stmt.query_map(time_params.as_slice(), |row| {
        Ok(DomainStat {
            domain: row.get(0)?,
            count: row.get(1)?,
        })
    })?;
    let mut domain_ranking = Vec::new();
    for r in domain_rows {
        domain_ranking.push(r?);
    }

    // 3. Browser breakdown
    let browser_sql = format!(
        r#"
        SELECT s.browser, count(v.id) as cnt
        FROM visits v
        JOIN urls u ON v.url_id = u.id
        JOIN sources s ON v.source_id = s.id
        WHERE v.visit_time >= ? AND v.visit_time <= ? {}
        GROUP BY s.browser
        ORDER BY cnt DESC
        "#,
        hidden_clause
    );
    let mut browser_stmt = conn.prepare(&browser_sql)?;
    let browser_rows = browser_stmt.query_map(time_params.as_slice(), |row| {
        let name: String = row.get(0)?;
        let count: u64 = row.get(1)?;
        let percent = if total_visits > 0 {
            ((count as f64 / total_visits as f64) * 100.0).round() as u32
        } else {
            0
        };
        Ok(BrowserStat {
            name,
            count,
            percent,
        })
    })?;
    let mut browser_breakdown = Vec::new();
    for r in browser_rows {
        browser_breakdown.push(r?);
    }

    // Derive all local-calendar distributions in one streamed pass over raw
    // integer timestamps (covering idx_visits_time_id when no hidden rules apply).
    let calendar_sql = if has_hidden {
        format!(
            "SELECT v.visit_time FROM visits v JOIN urls u ON v.url_id = u.id WHERE v.visit_time >= ? AND v.visit_time <= ? {}",
            hidden_clause
        )
    } else {
        "SELECT v.visit_time FROM visits v WHERE v.visit_time >= ? AND v.visit_time <= ?"
            .to_string()
    };
    let mut daily_counts = std::collections::BTreeMap::<(i32, u32, u32), u64>::new();
    let mut hourly_counts = [0u64; 24];
    let mut weekly_counts = [0u64; 168];
    {
        let mut statement = conn.prepare(&calendar_sql)?;
        let mut rows = statement.query(time_params.as_slice())?;
        let invalid_bucket = || AppError::Parse("本地时间统计结果无效".into());
        while let Some(row) = rows.next()? {
            let visit_ms: i64 = row.get(0)?;
            let dt = Local
                .timestamp_opt(visit_ms / 1000, 0)
                .earliest()
                .ok_or_else(invalid_bucket)?;
            let day = dt.weekday().num_days_from_sunday() as usize;
            let hour = dt.hour() as usize;
            let date_key = (dt.year(), dt.month(), dt.day());
            *daily_counts.entry(date_key).or_insert(0) += 1;
            hourly_counts[hour] += 1;
            weekly_counts[day * 24 + hour] += 1;
        }
    }
    let active_days = daily_counts.len() as u64;
    let daily_trend = daily_counts
        .into_iter()
        .map(|((y, m, d), count)| DailyTrendStat {
            date: format!("{y:04}-{m:02}-{d:02}"),
            count,
        })
        .collect();
    let hourly_distribution = hourly_counts
        .into_iter()
        .enumerate()
        .map(|(hour, count)| HourlyStat {
            hour: hour as u32,
            count,
        })
        .collect();
    let weekly_heatmap = weekly_counts
        .into_iter()
        .enumerate()
        .map(|(index, count)| WeeklyHeatmapPoint {
            day_of_week: (index / 24) as u32,
            hour: (index % 24) as u32,
            count,
        })
        .collect();

    // 6. Search queries (Top 20)
    let query_sql = format!(
        r#"
        SELECT u.url, v.visit_time
        FROM visits v
        JOIN urls u ON v.url_id = u.id
        WHERE v.visit_time >= ? AND v.visit_time <= ? {} AND (
            u.url LIKE '%search%' OR u.url LIKE '%/s?%' OR u.url LIKE '%google.%'
            OR u.url LIKE '%baidu.%' OR u.url LIKE '%bing.%' OR u.url LIKE '%duckduckgo.%'
            OR u.url LIKE '%bilibili.%' OR u.url LIKE '%zhihu.%' OR u.url LIKE '%sogou.%'
            OR u.url LIKE '%so.com%'
        )
        ORDER BY v.visit_time DESC
        LIMIT 2500
        "#,
        hidden_clause
    );
    let mut query_stmt = conn.prepare(&query_sql)?;
    let mut query_agg: std::collections::HashMap<(String, String), (u64, i64)> =
        std::collections::HashMap::new();
    let query_rows = query_stmt.query_map(time_params.as_slice(), |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    for (raw_url, vtime) in query_rows.flatten() {
        if let Some((engine, query_text)) = extract_search_query(&raw_url) {
            let entry = query_agg.entry((engine, query_text)).or_insert((0, vtime));
            entry.0 += 1;
            if vtime > entry.1 {
                entry.1 = vtime;
            }
        }
    }
    let mut top_search_queries: Vec<SearchQueryStat> = query_agg
        .into_iter()
        .map(
            |((engine, query), (count, last_searched_at))| SearchQueryStat {
                engine,
                query,
                count,
                last_searched_at,
            },
        )
        .collect();
    top_search_queries.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| b.last_searched_at.cmp(&a.last_searched_at))
    });
    top_search_queries.truncate(20);

    // 8. Forgotten Gems (高频访问 >= 3 次，但 30 天未再次打开)
    // Aggregate visits via covering idx_visits_url_time first so we avoid
    // joining urls before narrowing down to qualifying url_ids.
    let thirty_days_ago = now_ms - 30 * 86_400_000;
    let gems_sql = if has_hidden {
        format!(
            r#"
            SELECT u.id, COALESCE(u.title, ''), u.url, COALESCE(u.domain, ''), vc.cnt, vc.last_time
            FROM (
                SELECT url_id, count(*) AS cnt, max(visit_time) AS last_time
                FROM visits
                GROUP BY url_id
                HAVING cnt >= 3 AND last_time < ?1
            ) vc
            JOIN urls u ON u.id = vc.url_id
            WHERE 1=1 {hidden_clause}
            ORDER BY vc.cnt DESC, vc.last_time DESC
            LIMIT 15
            "#
        )
    } else {
        r#"
        SELECT u.id, COALESCE(u.title, ''), u.url, COALESCE(u.domain, ''), vc.cnt, vc.last_time
        FROM (
            SELECT url_id, count(*) AS cnt, max(visit_time) AS last_time
            FROM visits
            GROUP BY url_id
            HAVING cnt >= 3 AND last_time < ?1
            ORDER BY cnt DESC, last_time DESC
            LIMIT 15
        ) vc
        JOIN urls u ON u.id = vc.url_id
        ORDER BY vc.cnt DESC, vc.last_time DESC
        "#
        .to_string()
    };
    let mut gems_stmt = conn.prepare(&gems_sql)?;
    let mut gems_params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(1 + hidden_params.len());
    gems_params.push(&thirty_days_ago);
    for b in &hidden_params {
        gems_params.push(b.as_ref());
    }
    let gems_rows = gems_stmt.query_map(gems_params.as_slice(), |row| {
        Ok(ForgottenGemItem {
            url_id: row.get(0)?,
            title: row.get(1)?,
            url: row.get(2)?,
            domain: row.get(3)?,
            total_visits: row.get(4)?,
            last_visit_time: row.get(5)?,
        })
    })?;
    let mut forgotten_gems = Vec::new();
    for r in gems_rows {
        forgotten_gems.push(r?);
    }

    // 9. Period-over-Period comparison
    let duration = max_visit_time.saturating_sub(min_visit_time);
    let (previous_count, growth_rate) = if min_visit_time > 0 && duration > 0 {
        let prev_start = min_visit_time.saturating_sub(duration);
        let prev_end = min_visit_time;
        let mut prev_time_params: Vec<&dyn rusqlite::ToSql> =
            Vec::with_capacity(2 + hidden_params.len());
        prev_time_params.push(&prev_start);
        prev_time_params.push(&prev_end);
        for b in &hidden_params {
            prev_time_params.push(b.as_ref());
        }
        let prev_sql = if has_hidden {
            format!(
                "SELECT count(*) FROM visits v JOIN urls u ON v.url_id = u.id WHERE v.visit_time >= ? AND v.visit_time < ? {}",
                hidden_clause
            )
        } else {
            "SELECT count(*) FROM visits v WHERE v.visit_time >= ? AND v.visit_time < ?".to_string()
        };
        let prev_cnt: u64 = conn
            .query_row(&prev_sql, prev_time_params.as_slice(), |r| r.get(0))
            .unwrap_or(0);

        let rate = if prev_cnt > 0 {
            let diff = total_visits as f64 - prev_cnt as f64;
            ((diff / prev_cnt as f64) * 1000.0).round() / 10.0
        } else if total_visits > 0 {
            100.0
        } else {
            0.0
        };
        (prev_cnt, rate)
    } else {
        (0, 0.0)
    };

    let period_comparison = PeriodComparison {
        current_count: total_visits,
        previous_count,
        growth_rate,
    };

    Ok(AnalyticsSummary {
        total_visits,
        unique_urls,
        unique_domains,
        active_days,
        revisit_rate,
        domain_ranking,
        browser_breakdown,
        hourly_distribution,
        daily_trend,
        top_search_queries,
        weekly_heatmap,
        forgotten_gems,
        period_comparison,
    })
}

pub fn extract_search_query(url_str: &str) -> Option<(String, String)> {
    let parsed = url::Url::parse(url_str).ok()?;
    let host = parsed.host_str()?.to_lowercase();
    let path = parsed.path().to_lowercase();

    if host.contains("google.") && path.contains("/search") {
        for (k, v) in parsed.query_pairs() {
            if k == "q" {
                let trimmed = v.trim();
                if !trimmed.is_empty() {
                    return Some(("Google".to_string(), trimmed.to_string()));
                }
            }
        }
    } else if host.contains("baidu.com") && (path == "/s" || path == "/baidu") {
        for (k, v) in parsed.query_pairs() {
            if k == "wd" || k == "word" {
                let trimmed = v.trim();
                if !trimmed.is_empty() {
                    return Some(("Baidu".to_string(), trimmed.to_string()));
                }
            }
        }
    } else if host.contains("bing.com") && path.contains("/search") {
        for (k, v) in parsed.query_pairs() {
            if k == "q" {
                let trimmed = v.trim();
                if !trimmed.is_empty() {
                    return Some(("Bing".to_string(), trimmed.to_string()));
                }
            }
        }
    } else if host.contains("github.com") && path.contains("/search") {
        for (k, v) in parsed.query_pairs() {
            if k == "q" {
                let trimmed = v.trim();
                if !trimmed.is_empty() {
                    return Some(("GitHub".to_string(), trimmed.to_string()));
                }
            }
        }
    } else if host.contains("duckduckgo.com") {
        for (k, v) in parsed.query_pairs() {
            if k == "q" {
                let trimmed = v.trim();
                if !trimmed.is_empty() {
                    return Some(("DuckDuckGo".to_string(), trimmed.to_string()));
                }
            }
        }
    } else if host.contains("bilibili.com")
        && (path.contains("/search") || host.starts_with("search."))
    {
        for (k, v) in parsed.query_pairs() {
            if k == "keyword" {
                let trimmed = v.trim();
                if !trimmed.is_empty() {
                    return Some(("Bilibili".to_string(), trimmed.to_string()));
                }
            }
        }
    } else if host.contains("zhihu.com") && path.contains("/search") {
        for (k, v) in parsed.query_pairs() {
            if k == "q" {
                let trimmed = v.trim();
                if !trimmed.is_empty() {
                    return Some(("Zhihu".to_string(), trimmed.to_string()));
                }
            }
        }
    } else if host.contains("sogou.com") {
        for (k, v) in parsed.query_pairs() {
            if k == "query" {
                let trimmed = v.trim();
                if !trimmed.is_empty() {
                    return Some(("Sogou".to_string(), trimmed.to_string()));
                }
            }
        }
    } else if host.contains("so.com") && (path.contains("/s") || path.contains("/search")) {
        for (k, v) in parsed.query_pairs() {
            if k == "q" {
                let trimmed = v.trim();
                if !trimmed.is_empty() {
                    return Some(("360Search".to_string(), trimmed.to_string()));
                }
            }
        }
    }

    None
}

/// Returns locally visible research sessions. Hidden rules are excluded, while
/// Private rules remain available to local analytics and the session timeline.
pub fn get_research_sessions(
    conn: &Connection,
    days: Option<u32>,
    limit: u32,
) -> AppResult<Vec<ResearchSession>> {
    get_research_sessions_filtered(conn, days, limit, false)
}

/// Returns research sessions safe to summarize with a cloud AI provider.
/// Hidden and Private rules are both excluded (and legacy `block` rules are
/// treated as Private for backwards-compatible databases).
pub fn get_research_sessions_for_ai(
    conn: &Connection,
    days: Option<u32>,
    limit: u32,
) -> AppResult<Vec<ResearchSession>> {
    get_research_sessions_filtered(conn, days, limit, true)
}

fn get_research_sessions_filtered(
    conn: &Connection,
    days: Option<u32>,
    limit: u32,
    exclude_private: bool,
) -> AppResult<Vec<ResearchSession>> {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let min_visit_time = match days {
        Some(d) if d > 0 => now_ms - (d as i64) * 86_400_000,
        _ => 0,
    };

    let active_rules = get_active_privacy_rules(conn)?;
    struct ActiveSession {
        source_id: i64,
        start_time: i64,
        end_time: i64,
        visit_count: u32,
        domain_counts: std::collections::HashMap<String, u32>,
        earliest_title: String,
        browser: String,
        profile: String,
    }

    let mut sessions = std::collections::BTreeMap::new();
    let flush_session =
        |s: ActiveSession, out: &mut std::collections::BTreeMap<(i64, i64), ResearchSession>| {
            let mut dom_vec: Vec<_> = s.domain_counts.into_iter().collect();
            dom_vec.sort_by_key(|a| std::cmp::Reverse(a.1));
            let dominant_domain = dom_vec.first().map(|(d, _)| d.clone()).unwrap_or_default();
            let duration_secs = ((s.end_time - s.start_time) / 1000).max(0);
            let sample_title = if s.earliest_title.trim().is_empty() {
                dominant_domain.clone()
            } else {
                s.earliest_title
            };
            let session_id = format!("ses_{}_{}", s.source_id, s.start_time);

            out.insert(
                (s.start_time, s.source_id),
                ResearchSession {
                    session_id,
                    start_time: s.start_time,
                    end_time: s.end_time,
                    duration_secs,
                    visit_count: s.visit_count,
                    dominant_domain,
                    sample_title,
                    browser: s.browser,
                    profile: s.profile,
                },
            );
            if limit > 0 && out.len() > limit as usize {
                out.pop_first();
            }
        };

    // Order sources by latest activity so `sessions` fills with the newest entries
    // first and older sources can be pruned in one index seek.
    let mut source_stmt = conn.prepare(
        r#"
        SELECT s.id, s.browser, s.profile,
               (SELECT MAX(v.visit_time) FROM visits v WHERE v.source_id = s.id) AS max_t
        FROM sources s
        ORDER BY COALESCE(max_t, -9223372036854775808) DESC, s.id DESC
        "#,
    )?;
    let source_rows = source_stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<i64>>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    const SESSION_BREAK_MS: i64 = 30 * 60 * 1000; // 30 minutes
    let mut visit_stmt = conn.prepare_cached(
        r#"
        SELECT v.visit_time, COALESCE(u.title, ''), u.url, COALESCE(u.domain, '')
        FROM visits v
        JOIN urls u ON v.url_id = u.id
        WHERE v.source_id = ?1 AND v.visit_time >= ?2
        ORDER BY v.visit_time DESC
        "#,
    )?;

    for (source_id, browser, profile, max_visit_opt) in source_rows {
        let Some(max_t) = max_visit_opt else {
            continue;
        };
        if max_t < min_visit_time {
            continue;
        }
        if limit > 0 && sessions.len() >= limit as usize {
            if let Some(&(earliest_start, earliest_src)) = sessions.keys().next() {
                if max_t < earliest_start || (max_t == earliest_start && source_id < earliest_src) {
                    break;
                }
            }
        }

        let mut cur_session: Option<ActiveSession> = None;
        let mut rows = visit_stmt.query(params![source_id, min_visit_time])?;
        while let Some(row) = rows.next()? {
            let visit_time: i64 = row.get(0)?;
            let title: String = row.get(1)?;
            let url: String = row.get(2)?;
            let domain: String = row.get(3)?;
            let excluded = active_rules.iter().any(|rule| {
                let hidden = rule.rule_type == "hidden";
                // Unknown legacy rule types are treated as AI-blocking so a
                // malformed row can never widen cloud-data exposure.
                let private_for_ai = exclude_private && rule.rule_type != "hidden";
                (hidden || private_for_ai) && matches_privacy_rule(&rule.pattern, &domain, &url)
            });
            if excluded {
                continue;
            }

            match cur_session.take() {
                None => {
                    if limit > 0 && sessions.len() >= limit as usize {
                        if let Some(&(earliest_start, earliest_src)) = sessions.keys().next() {
                            if visit_time < earliest_start
                                || (visit_time == earliest_start && source_id < earliest_src)
                            {
                                break;
                            }
                        }
                    }
                    let mut dmap = std::collections::HashMap::new();
                    if !domain.is_empty() {
                        dmap.insert(domain, 1);
                    }
                    cur_session = Some(ActiveSession {
                        source_id,
                        start_time: visit_time,
                        end_time: visit_time,
                        visit_count: 1,
                        domain_counts: dmap,
                        earliest_title: title,
                        browser: browser.clone(),
                        profile: profile.clone(),
                    });
                }
                Some(mut s) => {
                    if (s.start_time - visit_time) <= SESSION_BREAK_MS {
                        if !domain.is_empty() {
                            *s.domain_counts.entry(domain).or_insert(0) += 1;
                        }
                        s.start_time = visit_time;
                        s.earliest_title = title;
                        s.visit_count += 1;
                        cur_session = Some(s);
                    } else {
                        flush_session(s, &mut sessions);
                        if limit > 0 && sessions.len() >= limit as usize {
                            if let Some(&(earliest_start, earliest_src)) = sessions.keys().next() {
                                if visit_time < earliest_start
                                    || (visit_time == earliest_start && source_id < earliest_src)
                                {
                                    break;
                                }
                            }
                        }
                        let mut new_dmap = std::collections::HashMap::new();
                        if !domain.is_empty() {
                            new_dmap.insert(domain, 1);
                        }
                        cur_session = Some(ActiveSession {
                            source_id,
                            start_time: visit_time,
                            end_time: visit_time,
                            visit_count: 1,
                            domain_counts: new_dmap,
                            earliest_title: title,
                            browser: browser.clone(),
                            profile: profile.clone(),
                        });
                    }
                }
            }
        }
        if let Some(last_s) = cur_session {
            flush_session(last_s, &mut sessions);
        }
    }

    Ok(sessions.into_values().rev().collect())
}

// ==========================================
// Security & App Lock Repository Methods
// ==========================================

pub fn get_security_state(conn: &Connection) -> AppResult<SecurityState> {
    let now = chrono::Utc::now().timestamp_millis();
    let has_envelope = get_master_key_envelope(conn)?.is_some();
    let row = conn.query_row(
        r#"
        SELECT pin_enabled, auto_lock_minutes, lock_on_minimize, lock_on_sleep,
               failed_attempts, locked_until
        FROM app_security WHERE id = 1
        "#,
        [],
        |r| {
            let pin_enabled: i64 = r.get(0)?;
            let auto_lock_minutes: i64 = r.get(1)?;
            let lock_on_minimize: i64 = r.get(2)?;
            let lock_on_sleep: i64 = r.get(3)?;
            let _failed_attempts: i64 = r.get(4)?;
            let locked_until: i64 = r.get(5)?;

            let is_locked_out = locked_until > now;
            let lockout_remaining_secs = if is_locked_out {
                ((locked_until - now + 999) / 1000).max(0)
            } else {
                0
            };

            let enabled = pin_enabled == 1;
            Ok(SecurityState {
                pin_enabled: enabled,
                auto_lock_minutes,
                lock_on_minimize: lock_on_minimize == 1,
                lock_on_sleep: lock_on_sleep == 1,
                native_sleep_lock_supported: crate::security::power::supported(),
                pin_migration_needed: enabled && !has_envelope,
                is_locked_out,
                lockout_remaining_secs,
            })
        },
    )?;

    Ok(row)
}

#[derive(Debug, Clone)]
pub struct SecurityCredentials {
    pub pin_enabled: bool,
    pub pin_salt: Option<String>,
    pub pin_hash: Option<String>,
    pub failed_attempts: i64,
    pub locked_until: i64,
}

pub fn get_security_credentials(conn: &Connection) -> AppResult<SecurityCredentials> {
    conn.query_row(
        r#"
        SELECT pin_enabled, pin_salt, pin_hash, failed_attempts, locked_until
        FROM app_security WHERE id = 1
        "#,
        [],
        |r| {
            let pin_enabled: i64 = r.get(0)?;
            let pin_salt: Option<String> = r.get(1)?;
            let pin_hash: Option<String> = r.get(2)?;
            let failed_attempts: i64 = r.get(3)?;
            let locked_until: i64 = r.get(4)?;
            Ok(SecurityCredentials {
                pin_enabled: pin_enabled == 1,
                pin_salt,
                pin_hash,
                failed_attempts,
                locked_until,
            })
        },
    )
    .map_err(Into::into)
}

pub fn update_pin(
    conn: &Connection,
    pin_hash: Option<&str>,
    pin_salt: Option<&str>,
) -> AppResult<()> {
    let now = chrono::Utc::now().timestamp_millis();
    conn.execute(
        r#"
        UPDATE app_security
        SET pin_enabled = 1,
            pin_hash = ?1,
            pin_salt = ?2,
            failed_attempts = 0,
            locked_until = 0,
            updated_at = ?3
        WHERE id = 1
        "#,
        params![pin_hash, pin_salt, now],
    )?;
    Ok(())
}

pub fn clear_pin(conn: &Connection) -> AppResult<()> {
    let now = chrono::Utc::now().timestamp_millis();
    conn.execute(
        r#"
        UPDATE app_security
        SET pin_enabled = 0,
            pin_hash = NULL,
            pin_salt = NULL,
            failed_attempts = 0,
            locked_until = 0,
            updated_at = ?1
        WHERE id = 1
        "#,
        params![now],
    )?;
    Ok(())
}

pub fn record_pin_failure(
    conn: &Connection,
    new_attempts: i64,
    locked_until: i64,
) -> AppResult<()> {
    let now = chrono::Utc::now().timestamp_millis();
    conn.execute(
        r#"
        UPDATE app_security
        SET failed_attempts = ?1,
            locked_until = ?2,
            updated_at = ?3
        WHERE id = 1
        "#,
        params![new_attempts, locked_until, now],
    )?;
    Ok(())
}

pub fn reset_pin_failures(conn: &Connection) -> AppResult<()> {
    let now = chrono::Utc::now().timestamp_millis();
    conn.execute(
        r#"
        UPDATE app_security
        SET failed_attempts = 0,
            locked_until = 0,
            updated_at = ?1
        WHERE id = 1
        "#,
        params![now],
    )?;
    Ok(())
}

pub fn update_security_options(
    conn: &Connection,
    auto_lock_minutes: i64,
    lock_on_minimize: bool,
    lock_on_sleep: bool,
) -> AppResult<()> {
    let now = chrono::Utc::now().timestamp_millis();
    conn.execute(
        r#"
        UPDATE app_security
        SET auto_lock_minutes = ?1,
            lock_on_minimize = ?2,
            lock_on_sleep = ?3,
            updated_at = ?4
        WHERE id = 1
        "#,
        params![
            auto_lock_minutes,
            if lock_on_minimize { 1 } else { 0 },
            if lock_on_sleep { 1 } else { 0 },
            now
        ],
    )?;
    Ok(())
}

// ==========================================
// Privacy Rules Repository Methods
// ==========================================

pub fn list_privacy_rules(conn: &Connection) -> AppResult<Vec<PrivacyRule>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, pattern, rule_type, enabled, created_at
        FROM privacy_rules
        ORDER BY id ASC
        "#,
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(PrivacyRule {
            id: row.get(0)?,
            pattern: row.get(1)?,
            rule_type: row.get(2)?,
            enabled: row.get::<_, i64>(3)? == 1,
            created_at: row.get(4)?,
        })
    })?;

    let mut rules = Vec::new();
    for r in rows {
        rules.push(r?);
    }
    Ok(rules)
}

pub fn get_active_privacy_rules(conn: &Connection) -> AppResult<Vec<PrivacyRule>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, pattern, rule_type, enabled, created_at
        FROM privacy_rules
        WHERE enabled = 1
        ORDER BY id ASC
        "#,
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(PrivacyRule {
            id: row.get(0)?,
            pattern: row.get(1)?,
            rule_type: row.get(2)?,
            enabled: row.get::<_, i64>(3)? == 1,
            created_at: row.get(4)?,
        })
    })?;

    let mut rules = Vec::new();
    for r in rows {
        rules.push(r?);
    }
    Ok(rules)
}

pub fn add_privacy_rule(
    conn: &Connection,
    pattern: &str,
    rule_type: &str,
) -> AppResult<PrivacyRule> {
    let now = chrono::Utc::now().timestamp_millis();
    let trimmed = pattern.trim().to_lowercase();
    if trimmed.is_empty() {
        return Err(AppError::Parse("规则匹配模式不能为空".to_string()));
    }
    let normalized_type = rule_type.trim().to_lowercase();
    if !matches!(normalized_type.as_str(), "private" | "hidden") {
        return Err(AppError::Parse(
            "规则类型必须是 private 或 hidden".to_string(),
        ));
    }
    conn.execute(
        r#"
        INSERT INTO privacy_rules (pattern, rule_type, enabled, created_at)
        VALUES (?1, ?2, 1, ?3)
        "#,
        params![trimmed, normalized_type, now],
    )?;
    let id = conn.last_insert_rowid();
    Ok(PrivacyRule {
        id,
        pattern: trimmed,
        rule_type: normalized_type,
        enabled: true,
        created_at: now,
    })
}

pub fn toggle_privacy_rule(conn: &Connection, id: i64, enabled: bool) -> AppResult<()> {
    conn.execute(
        "UPDATE privacy_rules SET enabled = ?1 WHERE id = ?2",
        params![if enabled { 1 } else { 0 }, id],
    )?;
    Ok(())
}

pub fn delete_privacy_rule(conn: &Connection, id: i64) -> AppResult<()> {
    conn.execute("DELETE FROM privacy_rules WHERE id = ?1", params![id])?;
    Ok(())
}

// ==========================================
// Backups Repository Methods
// ==========================================

pub fn record_backup(
    conn: &Connection,
    file_name: &str,
    file_path: &str,
    file_size_bytes: u64,
    notes: Option<&str>,
) -> AppResult<BackupInfo> {
    let now = chrono::Utc::now().timestamp_millis();
    conn.execute(
        r#"
        INSERT INTO backups (file_name, file_path, file_size_bytes, created_at, status, notes)
        VALUES (?1, ?2, ?3, ?4, 'success', ?5)
        "#,
        params![file_name, file_path, file_size_bytes as i64, now, notes],
    )?;
    let id = conn.last_insert_rowid();
    Ok(BackupInfo {
        id,
        file_name: file_name.to_string(),
        file_path: file_path.to_string(),
        file_size_bytes,
        created_at: now,
        notes: notes.map(|s| s.to_string()),
    })
}

pub fn list_backups(conn: &Connection) -> AppResult<Vec<BackupInfo>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, file_name, file_path, file_size_bytes, created_at, notes
        FROM backups
        ORDER BY created_at DESC
        "#,
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(BackupInfo {
            id: row.get(0)?,
            file_name: row.get(1)?,
            file_path: row.get(2)?,
            file_size_bytes: row.get::<_, i64>(3)? as u64,
            created_at: row.get(4)?,
            notes: row.get(5)?,
        })
    })?;

    let mut backups = Vec::new();
    for r in rows {
        backups.push(r?);
    }
    Ok(backups)
}

pub fn delete_backup(conn: &Connection, id: i64) -> AppResult<Option<String>> {
    let path: Option<String> = conn
        .query_row(
            "SELECT file_path FROM backups WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .optional()?;
    conn.execute("DELETE FROM backups WHERE id = ?1", params![id])?;
    Ok(path)
}

pub fn get_backup_by_id(conn: &Connection, id: i64) -> AppResult<Option<BackupInfo>> {
    let row = conn
        .query_row(
            "SELECT id, file_name, file_path, file_size_bytes, created_at, notes FROM backups WHERE id = ?1",
            params![id],
            |r| {
                Ok(BackupInfo {
                    id: r.get(0)?,
                    file_name: r.get(1)?,
                    file_path: r.get(2)?,
                    file_size_bytes: r.get::<_, i64>(3)? as u64,
                    created_at: r.get(4)?,
                    notes: r.get(5)?,
                })
            },
        )
        .optional()?;
    Ok(row)
}

pub fn get_master_key_envelope(conn: &Connection) -> AppResult<Option<String>> {
    let row: Option<String> = conn
        .query_row(
            "SELECT envelope_json FROM master_key_envelope WHERE id = 1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(row)
}

pub fn save_master_key_envelope(
    conn: &Connection,
    envelope_json: &str,
    recovery_key_hash: Option<&str>,
) -> AppResult<()> {
    let now = chrono::Utc::now().timestamp_millis();
    conn.execute(
        r#"
        INSERT INTO master_key_envelope (id, envelope_json, recovery_key_hash, updated_at)
        VALUES (1, ?1, ?2, ?3)
        ON CONFLICT(id) DO UPDATE SET
            envelope_json = excluded.envelope_json,
            recovery_key_hash = COALESCE(excluded.recovery_key_hash, recovery_key_hash),
            updated_at = excluded.updated_at
        "#,
        params![envelope_json, recovery_key_hash, now],
    )?;
    Ok(())
}

pub fn clear_master_key_envelope(conn: &Connection) -> AppResult<()> {
    conn.execute("DELETE FROM master_key_envelope WHERE id = 1", [])?;
    Ok(())
}

// ==========================================
// Maintenance & Integrity Repository Methods
// ==========================================

pub fn check_database_integrity(conn: &Connection) -> AppResult<IntegrityReport> {
    let integrity_message: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    let quick_check_message: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;

    let total_sources: u64 = conn.query_row("SELECT COUNT(*) FROM sources", [], |r| r.get(0))?;
    let total_urls: u64 = conn.query_row("SELECT COUNT(*) FROM urls", [], |r| r.get(0))?;
    let total_visits: u64 = conn.query_row("SELECT COUNT(*) FROM visits", [], |r| r.get(0))?;

    let fts_count: u64 = conn
        .query_row("SELECT COUNT(*) FROM urls_fts", [], |r| r.get(0))
        .unwrap_or(0);
    let fts_synced = fts_count == total_urls;

    let is_healthy = integrity_message == "ok" && quick_check_message == "ok";

    Ok(IntegrityReport {
        is_healthy,
        integrity_message,
        quick_check_message,
        total_sources,
        total_urls,
        total_visits,
        fts_synced,
    })
}

pub fn vacuum_database(conn: &Connection) -> AppResult<()> {
    conn.execute_batch("VACUUM;")?;
    Ok(())
}

// ==========================================
// AI Web Memory Context & Search Methods
// ==========================================

pub fn query_history_for_ai(
    conn: &Connection,
    query: &str,
    limit: u32,
) -> AppResult<Vec<WebMemoryCitation>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let rules = get_active_privacy_rules(conn)?;
    let mut conditions = Vec::new();
    let mut values = Vec::new();
    build_rule_conditions(
        &rules,
        DataScope::Ai,
        "u.domain",
        "u.url",
        &mut conditions,
        &mut values,
    );
    let privacy_clause = conditions.join(" AND ");
    let mut output = Vec::new();
    let mut seen_urls = std::collections::HashSet::new();
    // Stream ordered rows until enough distinct eligible URLs have been found.
    // A fixed visit LIMIT before deduplication starves older public evidence.
    let mut collect = |fts: bool, search: Option<String>| -> AppResult<()> {
        if output.len() >= limit as usize {
            return Ok(());
        }
        let mut params_ref: Vec<&dyn rusqlite::ToSql> = values.iter().map(|v| v.as_ref()).collect();
        let search_clause = match (fts, search.as_ref()) {
            (true, Some(value)) => {
                params_ref.push(value);
                "AND urls_fts MATCH ?"
            }
            (false, Some(value)) => {
                params_ref.extend([value as &dyn rusqlite::ToSql, value, value]);
                "AND (u.title LIKE ? ESCAPE '\\' OR u.url LIKE ? ESCAPE '\\' OR u.domain LIKE ? ESCAPE '\\')"
            }
            (_, None) => "",
        };
        let fts_join = if fts {
            "JOIN urls_fts ON urls_fts.rowid = u.id"
        } else {
            ""
        };
        let sql = format!(
            "SELECT v.id, COALESCE(u.title,''), u.url, COALESCE(u.domain,''), v.visit_time
             FROM visits v JOIN urls u ON v.url_id = u.id {fts_join}
             WHERE {privacy_clause} {search_clause}
             ORDER BY v.visit_time DESC, v.id DESC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(params_ref.as_slice())?;
        while let Some(row) = rows.next()? {
            let url: String = row.get(2)?;
            if seen_urls.insert(url.clone()) {
                output.push(WebMemoryCitation {
                    visit_id: row.get(0)?,
                    title: row.get(1)?,
                    url,
                    domain: row.get(3)?,
                    visit_time: row.get(4)?,
                });
                if output.len() >= limit as usize {
                    break;
                }
            }
        }
        Ok(())
    };
    let trimmed = query.trim();
    if trimmed.is_empty() {
        collect(false, None)?;
    } else {
        let cleaned: String = trimmed
            .chars()
            .filter(|c| c.is_alphanumeric() || c.is_whitespace() || matches!(c, '_' | '-' | '.'))
            .collect();
        let tokens: Vec<_> = cleaned
            .split_whitespace()
            .map(|word| format!("\"{word}\"*"))
            .collect();
        if !tokens.is_empty() {
            collect(true, Some(tokens.join(" ")))?;
        }
        // LIKE also supports languages/punctuation not tokenized as expected by FTS.
        // Reuse the same privacy and distinct-URL collection before bounding output.
        collect(false, Some(format!("%{}%", escape_like_literal(trimmed))))?;
    }
    output.truncate(limit as usize);
    Ok(output)
}

// ---------------------------------------------------------------------------
// Milestone C: Smart Collections, Domain Detail, Path Tree, Ranking & Dynamics
// ---------------------------------------------------------------------------

pub fn list_smart_collections(conn: &Connection) -> AppResult<Vec<SmartCollection>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, query, filter_json, created_at FROM smart_collections ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(SmartCollection {
            id: row.get(0)?,
            name: row.get(1)?,
            query: row.get(2)?,
            filter_json: row.get(3)?,
            created_at: row.get(4)?,
        })
    })?;
    let mut items = Vec::new();
    for r in rows {
        items.push(r?);
    }
    Ok(items)
}

pub fn create_smart_collection(
    conn: &Connection,
    name: &str,
    query: &str,
    filter_json: Option<&str>,
) -> AppResult<SmartCollection> {
    let now = chrono::Utc::now().timestamp_millis();
    conn.execute(
        "INSERT INTO smart_collections (name, query, filter_json, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![name, query, filter_json, now],
    )?;
    let id = conn.last_insert_rowid();
    Ok(SmartCollection {
        id,
        name: name.to_string(),
        query: query.to_string(),
        filter_json: filter_json.map(|s| s.to_string()),
        created_at: now,
    })
}

pub fn delete_smart_collection(conn: &Connection, id: i64) -> AppResult<()> {
    conn.execute("DELETE FROM smart_collections WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn get_website_ranking(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    limit: Option<u32>,
) -> AppResult<Vec<WebsiteRankingItem>> {
    use chrono::{Datelike, Local, TimeZone};

    let now_ms = chrono::Utc::now().timestamp_millis();
    let min_time = start_time.unwrap_or(0);
    let max_time = end_time.unwrap_or(now_ms);
    let top_limit = limit.unwrap_or(20).min(100) as i64;

    let active_rules = get_active_privacy_rules(conn)?;
    let mut hidden_conds = Vec::new();
    let mut hidden_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    build_hidden_rule_conditions(
        &active_rules,
        "u.domain",
        "u.url",
        &mut hidden_conds,
        &mut hidden_params,
    );
    let hidden_clause = if hidden_conds.is_empty() {
        String::new()
    } else {
        format!("AND {}", hidden_conds.join(" AND "))
    };

    // Stage 1: rank top domains without computing strftime across all rows.
    // For full-history queries, pre-aggregate via covering idx_visits_url_time.
    struct RawDomainRank {
        domain: String,
        visits: u64,
        unique_urls: u64,
        first_visit_time: i64,
        last_visit_time: i64,
    }
    let mut ranked: Vec<RawDomainRank> = if start_time.is_none() && end_time.is_none() {
        let sql = format!(
            r#"
            SELECT u.domain, SUM(vc.cnt) as visits, COUNT(u.id) as unique_urls,
                   MIN(vc.min_t) as first_visit, MAX(vc.max_t) as last_visit
            FROM (
                SELECT url_id, COUNT(*) AS cnt, MIN(visit_time) AS min_t, MAX(visit_time) AS max_t
                FROM visits
                GROUP BY url_id
            ) vc
            JOIN urls u ON u.id = vc.url_id
            WHERE u.domain IS NOT NULL AND u.domain != '' {}
            GROUP BY u.domain
            ORDER BY visits DESC
            LIMIT ?
            "#,
            hidden_clause
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut qparams: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(1 + hidden_params.len());
        for p in &hidden_params {
            qparams.push(p.as_ref());
        }
        qparams.push(&top_limit);
        let rows = stmt.query_map(qparams.as_slice(), |row| {
            Ok(RawDomainRank {
                domain: row.get(0)?,
                visits: row.get(1)?,
                unique_urls: row.get(2)?,
                first_visit_time: row.get(3)?,
                last_visit_time: row.get(4)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    } else {
        let sql = format!(
            r#"
            SELECT u.domain, count(v.id) as visits, count(DISTINCT u.id) as unique_urls,
                   min(v.visit_time) as first_visit, max(v.visit_time) as last_visit
            FROM visits v
            JOIN urls u ON v.url_id = u.id
            WHERE v.visit_time >= ? AND v.visit_time <= ? AND u.domain IS NOT NULL AND u.domain != '' {}
            GROUP BY u.domain
            ORDER BY visits DESC
            LIMIT ?
            "#,
            hidden_clause
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut qparams: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(3 + hidden_params.len());
        qparams.push(&min_time);
        qparams.push(&max_time);
        for p in &hidden_params {
            qparams.push(p.as_ref());
        }
        qparams.push(&top_limit);
        let rows = stmt.query_map(qparams.as_slice(), |row| {
            Ok(RawDomainRank {
                domain: row.get(0)?,
                visits: row.get(1)?,
                unique_urls: row.get(2)?,
                first_visit_time: row.get(3)?,
                last_visit_time: row.get(4)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };

    if ranked.is_empty() {
        return Ok(Vec::new());
    }

    // Stage 2: compute active_days only for the selected top domains.
    let placeholders = vec!["?"; ranked.len()].join(",");
    let unbounded_range = start_time.is_none() && end_time.is_none();
    let days_sql = if unbounded_range {
        format!(
            r#"
            SELECT u.domain, v.visit_time
            FROM urls u
            JOIN visits v ON v.url_id = u.id
            WHERE u.domain IN ({}) {}
            "#,
            placeholders, hidden_clause
        )
    } else {
        format!(
            r#"
            SELECT u.domain, v.visit_time
            FROM urls u
            JOIN visits v ON v.url_id = u.id
            WHERE u.domain IN ({}) AND v.visit_time >= ? AND v.visit_time <= ? {}
            "#,
            placeholders, hidden_clause
        )
    };
    let mut days_params: Vec<&dyn rusqlite::ToSql> =
        Vec::with_capacity(ranked.len() + 2 + hidden_params.len());
    for item in &ranked {
        days_params.push(&item.domain);
    }
    if !unbounded_range {
        days_params.push(&min_time);
        days_params.push(&max_time);
    }
    for p in &hidden_params {
        days_params.push(p.as_ref());
    }
    let mut active_days_map: std::collections::HashMap<
        String,
        std::collections::HashSet<(i32, u32, u32)>,
    > = std::collections::HashMap::with_capacity(ranked.len());
    let mut days_stmt = conn.prepare(&days_sql)?;
    let mut days_rows = days_stmt.query(days_params.as_slice())?;
    while let Some(row) = days_rows.next()? {
        let domain: String = row.get(0)?;
        let visit_ms: i64 = row.get(1)?;
        if let Some(dt) = Local.timestamp_opt(visit_ms / 1000, 0).earliest() {
            active_days_map
                .entry(domain)
                .or_default()
                .insert((dt.year(), dt.month(), dt.day()));
        }
    }

    let list = ranked
        .drain(..)
        .map(|item| {
            let active_days = active_days_map
                .get(&item.domain)
                .map(|set| set.len() as u64)
                .unwrap_or(0);
            let revisit_rate = if item.visits > 0 {
                ((item.visits as f64 - item.unique_urls as f64) / item.visits as f64 * 1000.0)
                    .round()
                    / 10.0
            } else {
                0.0
            };
            WebsiteRankingItem {
                domain: item.domain,
                visits: item.visits,
                unique_urls: item.unique_urls,
                active_days,
                revisit_rate: revisit_rate.max(0.0),
                first_visit_time: item.first_visit_time,
                last_visit_time: item.last_visit_time,
            }
        })
        .collect();

    Ok(list)
}

fn build_path_tree(urls_with_counts: Vec<(String, u64)>) -> Vec<PathTreeNode> {
    use std::collections::{HashMap, HashSet};

    struct InternalNode {
        name: String,
        full_path: String,
        visits: u64,
        urls: HashSet<String>,
        children: HashMap<String, InternalNode>,
    }

    let mut root_children: HashMap<String, InternalNode> = HashMap::new();

    for (raw_url, count) in urls_with_counts {
        let path = if let Ok(parsed) = url::Url::parse(&raw_url) {
            parsed.path().to_string()
        } else {
            continue;
        };

        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).take(3).collect();
        if segments.is_empty() {
            continue;
        }

        let mut current_map = &mut root_children;
        let mut path_acc = String::new();

        for seg in segments {
            path_acc.push('/');
            path_acc.push_str(seg);

            let node = current_map
                .entry(seg.to_string())
                .or_insert_with(|| InternalNode {
                    name: seg.to_string(),
                    full_path: path_acc.clone(),
                    visits: 0,
                    urls: HashSet::new(),
                    children: HashMap::new(),
                });

            node.visits += count;
            node.urls.insert(raw_url.clone());
            current_map = &mut node.children;
        }
    }

    fn convert(node: InternalNode) -> PathTreeNode {
        let mut children: Vec<PathTreeNode> = node.children.into_values().map(convert).collect();
        children.sort_by_key(|a| std::cmp::Reverse(a.visits));
        PathTreeNode {
            name: node.name,
            full_path: node.full_path,
            visits: node.visits,
            unique_urls: node.urls.len() as u64,
            children,
        }
    }

    let mut result: Vec<PathTreeNode> = root_children.into_values().map(convert).collect();
    result.sort_by_key(|a| std::cmp::Reverse(a.visits));
    result
}

pub fn get_domain_detail(
    conn: &Connection,
    domain: &str,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> AppResult<DomainDetail> {
    let active_rules = get_active_privacy_rules(conn)?;
    let hidden_rules: Vec<_> = active_rules
        .iter()
        .filter(|r| r.rule_type == "hidden")
        .collect();
    if hidden_rules
        .iter()
        .any(|r| matches_privacy_rule(&r.pattern, domain, ""))
    {
        return Ok(DomainDetail {
            domain: domain.to_string(),
            total_visits: 0,
            unique_urls: 0,
            first_visit_time: 0,
            last_visit_time: 0,
            hourly_distribution: (0..24).map(|h| HourlyStat { hour: h, count: 0 }).collect(),
            top_urls: Vec::new(),
            path_tree: Vec::new(),
        });
    }

    let now_ms = chrono::Utc::now().timestamp_millis();
    let min_time = start_time.unwrap_or(0);
    let max_time = end_time.unwrap_or(now_ms);

    let mut hidden_conds = Vec::new();
    let mut hidden_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    build_hidden_rule_conditions(
        &active_rules,
        "u.domain",
        "u.url",
        &mut hidden_conds,
        &mut hidden_params,
    );
    let hidden_clause = if hidden_conds.is_empty() {
        String::new()
    } else {
        format!("AND {}", hidden_conds.join(" AND "))
    };
    // 1. Overview
    let overview: Option<(u64, u64, Option<i64>, Option<i64>)> = conn
        .query_row(
            &format!(
                r#"
            SELECT count(v.id), count(DISTINCT u.id), min(v.visit_time), max(v.visit_time)
            FROM visits v
            JOIN urls u ON v.url_id = u.id
            WHERE u.domain = ? AND v.visit_time >= ? AND v.visit_time <= ? {}
            "#,
                hidden_clause
            ),
            {
                let mut params: Vec<&dyn rusqlite::ToSql> =
                    Vec::with_capacity(3 + hidden_params.len());
                params.push(&domain);
                params.push(&min_time);
                params.push(&max_time);
                for p in &hidden_params {
                    params.push(p.as_ref());
                }
                params
            }
            .as_slice(),
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;

    let (total_visits, unique_urls, first_visit_time, last_visit_time) = match overview {
        Some((tv, uu, fv, lv)) => (tv, uu, fv.unwrap_or(0), lv.unwrap_or(0)),
        None => (0, 0, 0, 0),
    };

    // 2. Hourly distribution
    let mut hourly_map = std::collections::HashMap::new();
    let hourly_sql = format!(
        r#"
        SELECT CAST(strftime('%H', datetime(v.visit_time / 1000, 'unixepoch', 'localtime')) AS INTEGER) as hr, count(v.id)
        FROM visits v
        JOIN urls u ON v.url_id = u.id
        WHERE u.domain = ? AND v.visit_time >= ? AND v.visit_time <= ? {}
        GROUP BY hr
        "#,
        hidden_clause
    );
    let mut hourly_stmt = conn.prepare(&hourly_sql)?;
    let mut hourly_params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(3 + hidden_params.len());
    hourly_params.push(&domain);
    hourly_params.push(&min_time);
    hourly_params.push(&max_time);
    for p in &hidden_params {
        hourly_params.push(p.as_ref());
    }
    let h_rows = hourly_stmt.query_map(hourly_params.as_slice(), |row| {
        Ok((row.get::<_, u32>(0)?, row.get::<_, u64>(1)?))
    })?;
    for (hr, cnt) in h_rows.flatten() {
        hourly_map.insert(hr, cnt);
    }
    let mut hourly_distribution = Vec::with_capacity(24);
    for h in 0..24 {
        hourly_distribution.push(HourlyStat {
            hour: h,
            count: *hourly_map.get(&h).unwrap_or(&0),
        });
    }

    // 3. Top URLs
    let top_urls_sql = format!(
        r#"
        SELECT u.url, COALESCE(u.title, ''), count(v.id) as cnt, max(v.visit_time) as last_time
        FROM visits v
        JOIN urls u ON v.url_id = u.id
        WHERE u.domain = ? AND v.visit_time >= ? AND v.visit_time <= ? {}
        GROUP BY u.id
        ORDER BY cnt DESC, last_time DESC
        LIMIT 20
        "#,
        hidden_clause
    );
    let mut top_urls_stmt = conn.prepare(&top_urls_sql)?;
    let top_rows = top_urls_stmt.query_map(hourly_params.as_slice(), |row| {
        Ok(DomainTopUrl {
            url: row.get(0)?,
            title: row.get(1)?,
            visits: row.get(2)?,
            last_visit_time: row.get(3)?,
        })
    })?;
    let mut top_urls = Vec::new();
    for r in top_rows {
        let item = r?;
        top_urls.push(item);
    }

    // 4. URL Path Tree
    let tree_urls_sql = format!(
        r#"
        SELECT u.url, count(v.id) as cnt
        FROM visits v
        JOIN urls u ON v.url_id = u.id
        WHERE u.domain = ? AND v.visit_time >= ? AND v.visit_time <= ? {}
        GROUP BY u.id
        LIMIT 500
        "#,
        hidden_clause
    );
    let mut tree_urls_stmt = conn.prepare(&tree_urls_sql)?;
    let tree_rows = tree_urls_stmt.query_map(hourly_params.as_slice(), |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
    })?;
    let mut all_domain_urls = Vec::new();
    for (u, cnt) in tree_rows.flatten() {
        all_domain_urls.push((u, cnt));
    }
    let path_tree = build_path_tree(all_domain_urls);

    Ok(DomainDetail {
        domain: domain.to_string(),
        total_visits,
        unique_urls,
        first_visit_time,
        last_visit_time,
        hourly_distribution,
        top_urls,
        path_tree,
    })
}

pub fn get_domain_dynamics(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> AppResult<DomainDynamics> {
    let active_rules = get_active_privacy_rules(conn)?;
    let mut hidden_conds = Vec::new();
    let mut hidden_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    build_hidden_rule_conditions(
        &active_rules,
        "u.domain",
        "u.url",
        &mut hidden_conds,
        &mut hidden_params,
    );
    let hidden_clause = if hidden_conds.is_empty() {
        String::new()
    } else {
        format!("AND {}", hidden_conds.join(" AND "))
    };

    let now_ms = chrono::Utc::now().timestamp_millis();
    let min_time = start_time.unwrap_or_else(|| now_ms - 30 * 86_400_000);
    let max_time = end_time.unwrap_or(now_ms);

    // Aggregate visits per url_id using covering idx_visits_url_time once,
    // then fold into domain-level metrics for both new & dormant domains.
    let sql = format!(
        r#"
        SELECT u.domain, COALESCE(u.title, ''), u.url, vc.cnt, vc.min_t, vc.max_t
        FROM (
            SELECT url_id, count(*) AS cnt, min(visit_time) AS min_t, max(visit_time) AS max_t
            FROM visits
            GROUP BY url_id
        ) vc
        JOIN urls u ON u.id = vc.url_id
        WHERE u.domain IS NOT NULL AND u.domain != '' {}
        "#,
        hidden_clause
    );
    let mut stmt = conn.prepare(&sql)?;
    let params_ref: Vec<&dyn rusqlite::ToSql> = hidden_params.iter().map(|p| p.as_ref()).collect();

    struct DomainAgg {
        total_cnt: u64,
        first_visit: i64,
        last_visit: i64,
        sample_url: String,
        sample_title: String,
        best_url_cnt: u64,
    }

    let mut domains: std::collections::HashMap<String, DomainAgg> =
        std::collections::HashMap::new();
    let mut rows = stmt.query(params_ref.as_slice())?;
    while let Some(row) = rows.next()? {
        let domain: String = row.get(0)?;
        let title: String = row.get(1)?;
        let url: String = row.get(2)?;
        let cnt: u64 = row.get(3)?;
        let min_t: i64 = row.get(4)?;
        let max_t: i64 = row.get(5)?;

        domains
            .entry(domain)
            .and_modify(|agg| {
                agg.total_cnt += cnt;
                if min_t < agg.first_visit {
                    agg.first_visit = min_t;
                }
                if max_t > agg.last_visit {
                    agg.last_visit = max_t;
                }
                if cnt > agg.best_url_cnt {
                    agg.best_url_cnt = cnt;
                    agg.sample_url = url.clone();
                    agg.sample_title = title.clone();
                }
            })
            .or_insert_with(|| DomainAgg {
                total_cnt: cnt,
                first_visit: min_t,
                last_visit: max_t,
                sample_url: url,
                sample_title: title,
                best_url_cnt: cnt,
            });
    }

    let mut new_domains = Vec::new();
    let mut dormant_domains = Vec::new();
    for (domain, agg) in domains {
        if agg.first_visit >= min_time && agg.first_visit <= max_time {
            new_domains.push(NewDomainItem {
                domain,
                first_visit_time: agg.first_visit,
                sample_title: agg.sample_title,
                sample_url: agg.sample_url,
                visits_in_period: agg.total_cnt,
            });
        } else if agg.total_cnt >= 5 && agg.last_visit < min_time {
            let days_dormant = if now_ms > agg.last_visit {
                ((now_ms - agg.last_visit) / 86_400_000) as u64
            } else {
                0
            };
            dormant_domains.push(DormantDomainItem {
                domain,
                total_historical_visits: agg.total_cnt,
                last_visit_time: agg.last_visit,
                days_dormant,
            });
        }
    }

    new_domains.sort_by(|a, b| {
        b.visits_in_period
            .cmp(&a.visits_in_period)
            .then_with(|| b.first_visit_time.cmp(&a.first_visit_time))
    });
    new_domains.truncate(20);

    dormant_domains.sort_by(|a, b| {
        b.total_historical_visits
            .cmp(&a.total_historical_visits)
            .then_with(|| b.last_visit_time.cmp(&a.last_visit_time))
    });
    dormant_domains.truncate(20);

    Ok(DomainDynamics {
        new_domains,
        dormant_domains,
    })
}

fn local_day_millis_bounds(year: i32, month: u32, day: u32) -> Option<(i64, i64)> {
    use chrono::{Local, NaiveDate, TimeZone};
    let date = NaiveDate::from_ymd_opt(year, month, day)?;
    let next = date.succ_opt()?;
    let start_ms = Local
        .from_local_datetime(&date.and_hms_opt(0, 0, 0)?)
        .earliest()?
        .timestamp_millis();
    let next_ms = Local
        .from_local_datetime(&next.and_hms_opt(0, 0, 0)?)
        .earliest()?
        .timestamp_millis();
    Some((start_ms, next_ms - 1))
}

pub fn get_on_this_day(
    conn: &Connection,
    target_date: Option<String>,
    filter_year: Option<i32>,
    page: Option<u32>,
    page_size: Option<u32>,
) -> AppResult<OnThisDayResult> {
    use chrono::{Datelike, Local, TimeZone};

    let active_rules = get_active_privacy_rules(conn)?;
    let mut hidden_conds = Vec::new();
    let mut hidden_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    build_hidden_rule_conditions(
        &active_rules,
        "u.domain",
        "u.url",
        &mut hidden_conds,
        &mut hidden_params,
    );
    let has_hidden = !hidden_conds.is_empty();
    let hidden_clause = if !has_hidden {
        String::new()
    } else {
        format!("AND {}", hidden_conds.join(" AND "))
    };

    let date_str = target_date.unwrap_or_else(|| Local::now().format("%Y-%m-%d").to_string());
    let parsed_date = chrono::NaiveDate::parse_from_str(date_str.trim(), "%Y-%m-%d")
        .map_err(|_| AppError::Parse("无效的日期格式，应为 YYYY-MM-DD".into()))?;
    let target_year = parsed_date.year();
    let target_month = parsed_date.month();
    let target_day = parsed_date.day();
    let max_allowed_year = Local::now().year() + 1;
    if !(1970..=max_allowed_year).contains(&target_year) {
        return Err(AppError::Parse("查询年份超出合法范围".into()));
    }

    let p = page.unwrap_or(1).max(1);
    let ps = page_size.unwrap_or(30).clamp(1, 100);
    let offset = (p - 1) * ps;

    let min_visit_opt: Option<i64> =
        conn.query_row("SELECT MIN(visit_time) FROM visits", [], |r| r.get(0))?;
    let Some(min_visit_ms) = min_visit_opt else {
        return Ok(OnThisDayResult {
            items: Vec::new(),
            total_count: 0,
            available_years: Vec::new(),
            page: p,
            page_size: ps,
            total_pages: 1,
        });
    };

    let min_year = Local
        .timestamp_opt(min_visit_ms / 1000, 0)
        .earliest()
        .map(|dt| dt.year())
        .unwrap_or(1970)
        .max(1970);

    // Probe each candidate year using idx_visits_time_id range bounds instead of
    // scanning the entire table with strftime().
    let count_sql = if has_hidden {
        format!(
            "SELECT COUNT(*) FROM visits v JOIN urls u ON v.url_id = u.id WHERE v.visit_time >= ? AND v.visit_time <= ? {}",
            hidden_clause
        )
    } else {
        "SELECT COUNT(*) FROM visits v WHERE v.visit_time >= ? AND v.visit_time <= ?".to_string()
    };
    let mut count_stmt = conn.prepare(&count_sql)?;

    let mut available_years = Vec::new();
    let mut year_buckets: Vec<(i32, u32, i64, i64)> = Vec::new();
    for yr in (min_year..target_year).rev() {
        let Some((start_ms, end_ms)) = local_day_millis_bounds(yr, target_month, target_day) else {
            continue;
        };
        let mut count_params: Vec<&dyn rusqlite::ToSql> =
            Vec::with_capacity(2 + hidden_params.len());
        count_params.push(&start_ms);
        count_params.push(&end_ms);
        for b in &hidden_params {
            count_params.push(b.as_ref());
        }
        let cnt: u32 = count_stmt.query_row(count_params.as_slice(), |row| row.get(0))?;
        if cnt > 0 {
            available_years.push(yr);
            year_buckets.push((yr, cnt, start_ms, end_ms));
        }
    }

    let matching_buckets: Vec<&(i32, u32, i64, i64)> = match filter_year {
        Some(fy) => year_buckets.iter().filter(|b| b.0 == fy).collect(),
        None => year_buckets.iter().collect(),
    };
    let total_count: u32 = matching_buckets.iter().map(|b| b.1).sum();
    let total_pages = if total_count == 0 {
        1
    } else {
        total_count.div_ceil(ps)
    };

    let page_sql = format!(
        r#"
        SELECT v.id, u.url, COALESCE(u.title, '') as title, COALESCE(u.domain, '') as domain,
               v.visit_time
        FROM visits v
        JOIN urls u ON v.url_id = u.id
        WHERE v.visit_time >= ? AND v.visit_time <= ? {}
        ORDER BY v.visit_time DESC
        LIMIT ? OFFSET ?
        "#,
        hidden_clause
    );
    let mut page_stmt = conn.prepare(&page_sql)?;

    let mut items = Vec::new();
    let mut remaining_offset = offset;
    for &(yr, cnt, start_ms, end_ms) in matching_buckets {
        if items.len() >= ps as usize {
            break;
        }
        if remaining_offset >= cnt {
            remaining_offset -= cnt;
            continue;
        }
        let need = (ps as usize - items.len()) as u32;
        let local_offset = remaining_offset;
        remaining_offset = 0;

        let mut page_params: Vec<&dyn rusqlite::ToSql> =
            Vec::with_capacity(4 + hidden_params.len());
        page_params.push(&start_ms);
        page_params.push(&end_ms);
        for b in &hidden_params {
            page_params.push(b.as_ref());
        }
        page_params.push(&need);
        page_params.push(&local_offset);

        let years_ago = (target_year - yr).max(1) as u32;
        let rows = page_stmt.query_map(page_params.as_slice(), |row| {
            Ok(OnThisDayItem {
                id: row.get(0)?,
                url: row.get(1)?,
                title: row.get(2)?,
                domain: row.get(3)?,
                visit_time: row.get(4)?,
                years_ago,
            })
        })?;
        for r in rows {
            items.push(r?);
        }
    }

    Ok(OnThisDayResult {
        items,
        total_count,
        available_years,
        page: p,
        page_size: ps,
        total_pages,
    })
}

// ==========================================
// Milestone D: Embedding & Hybrid Search
// ==========================================

pub fn save_page_embeddings(
    conn: &Connection,
    records: &[(i64, String, Vec<f32>, &str)],
) -> AppResult<usize> {
    let now = chrono::Utc::now().timestamp_millis();
    let mut stmt = conn.prepare_cached(
        r#"
        INSERT INTO page_embeddings (url_id, text_sample, embedding, dimension, model, created_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ON CONFLICT(url_id, model) DO UPDATE SET
            text_sample = excluded.text_sample,
            embedding = excluded.embedding,
            dimension = excluded.dimension,
            created_at = excluded.created_at
        "#,
    )?;

    let mut count = 0;
    for (url_id, text_sample, floats, model) in records {
        let bytes = crate::ai::embedding::floats_to_bytes(floats);
        let dim = floats.len() as i64;
        stmt.execute(params![url_id, text_sample, bytes, dim, model, now])?;
        count += 1;
    }
    Ok(count)
}

pub fn get_unindexed_urls(
    conn: &Connection,
    model: &str,
    limit: usize,
) -> AppResult<Vec<(i64, String, String, String)>> {
    let mut exclude_conditions = Vec::new();
    let mut param_values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    param_values.push(Box::new(model.to_string()));

    let rules = get_active_privacy_rules(conn)?;
    build_rule_conditions(
        &rules,
        DataScope::Ai,
        "u.domain",
        "u.url",
        &mut exclude_conditions,
        &mut param_values,
    );

    let extra_where = if exclude_conditions.is_empty() {
        String::new()
    } else {
        format!("AND {}", exclude_conditions.join(" AND "))
    };

    let sql = format!(
        r#"
        SELECT u.id, u.url, COALESCE(u.title, ''), COALESCE(u.domain, '')
        FROM urls u
        LEFT JOIN page_embeddings pe ON u.id = pe.url_id AND pe.model = ?1
        WHERE pe.id IS NULL {}
        ORDER BY (SELECT count(*) FROM visits v WHERE v.url_id = u.id) DESC, u.id DESC
        LIMIT ?
        "#,
        extra_where
    );
    param_values.push(Box::new(limit));

    let mut stmt = conn.prepare(&sql)?;
    let rusqlite_params: Vec<&dyn rusqlite::ToSql> =
        param_values.iter().map(|b| b.as_ref()).collect();
    let rows = stmt.query_map(rusqlite_params.as_slice(), |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
    })?;

    let mut list = Vec::new();
    for r in rows {
        list.push(r?);
    }
    Ok(list)
}

pub fn get_embedding_indexing_status(
    conn: &Connection,
    model: &str,
) -> AppResult<EmbeddingIndexingStatus> {
    let mut exclude_conditions = Vec::new();
    let mut param_values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    let rules = get_active_privacy_rules(conn)?;
    build_rule_conditions(
        &rules,
        DataScope::Ai,
        "u.domain",
        "u.url",
        &mut exclude_conditions,
        &mut param_values,
    );

    let where_clause = if exclude_conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", exclude_conditions.join(" AND "))
    };

    let sql = format!(
        "SELECT COUNT(*), COALESCE(SUM(EXISTS(SELECT 1 FROM page_embeddings pe WHERE pe.url_id=u.id AND pe.model=?)),0)
         FROM urls u {where_clause}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut params_ref: Vec<&dyn rusqlite::ToSql> = vec![&model];
    params_ref.extend(param_values.iter().map(|value| value.as_ref()));
    let (total_urls, indexed_urls): (u64, u64) =
        stmt.query_row(params_ref.as_slice(), |row| Ok((row.get(0)?, row.get(1)?)))?;

    let pending_urls = total_urls.saturating_sub(indexed_urls);

    Ok(EmbeddingIndexingStatus {
        total_urls,
        indexed_urls,
        pending_urls,
        model: model.to_string(),
    })
}

pub fn get_url_embedding(
    conn: &Connection,
    url_id: i64,
    model: &str,
) -> AppResult<Option<Vec<f32>>> {
    let res = conn.query_row(
        "SELECT embedding FROM page_embeddings WHERE url_id = ?1 AND model = ?2",
        params![url_id, model],
        |r| {
            let bytes: Vec<u8> = r.get(0)?;
            Ok(crate::ai::embedding::bytes_to_floats(&bytes))
        },
    );

    match res {
        Ok(v) => Ok(Some(v)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(AppError::from(e)),
    }
}

pub fn find_similar_pages(
    conn: &Connection,
    url_id: i64,
    model: &str,
    limit: usize,
) -> AppResult<Vec<SimilarPageItem>> {
    let target_embedding = match get_url_embedding(conn, url_id, model)? {
        Some(emb) => emb,
        None => return Ok(Vec::new()),
    };

    let active_rules = get_active_privacy_rules(conn)?;
    let mut ai_conds = Vec::new();
    let mut ai_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    build_rule_conditions(
        &active_rules,
        DataScope::Ai,
        "u.domain",
        "u.url",
        &mut ai_conds,
        &mut ai_params,
    );
    let ai_clause = if ai_conds.is_empty() {
        String::new()
    } else {
        format!("AND {}", ai_conds.join(" AND "))
    };

    let sql = format!(
        r#"
        SELECT pe.url_id, u.url, COALESCE(u.title, ''), COALESCE(u.domain, ''),
               (SELECT count(*) FROM visits v WHERE v.url_id = u.id) as vc,
               COALESCE((SELECT max(v.visit_time) FROM visits v WHERE v.url_id = u.id), 0) as lvt,
               pe.embedding
        FROM page_embeddings pe
        JOIN urls u ON pe.url_id = u.id
        WHERE pe.model = ?1 AND pe.url_id != ?2 {ai_clause}
        ORDER BY lvt DESC
        LIMIT 4000
        "#
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut query_params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(2 + ai_params.len());
    query_params.push(&model);
    query_params.push(&url_id);
    for p in &ai_params {
        query_params.push(p.as_ref());
    }

    let rows = stmt.query_map(query_params.as_slice(), |r| {
        let u_id: i64 = r.get(0)?;
        let url: String = r.get(1)?;
        let title: String = r.get(2)?;
        let domain: String = r.get(3)?;
        let visit_count: u64 = r.get(4)?;
        let last_visit_time: i64 = r.get(5)?;
        let bytes: Vec<u8> = r.get(6)?;
        Ok((
            u_id,
            url,
            title,
            domain,
            visit_count,
            last_visit_time,
            bytes,
        ))
    })?;

    let mut scored = Vec::new();
    for r in rows {
        let (u_id, url, title, domain, vc, lvt, bytes) = r?;
        if active_rules
            .iter()
            .any(|rule| matches_privacy_rule(&rule.pattern, &domain, &url))
        {
            continue;
        }
        let emb = crate::ai::embedding::bytes_to_floats(&bytes);
        let sim = crate::ai::embedding::cosine_similarity(&target_embedding, &emb) as f64;
        if sim > 0.25 {
            scored.push(SimilarPageItem {
                url_id: u_id,
                url,
                title,
                domain,
                visit_count: vc,
                last_visit_time: lvt,
                similarity: (sim * 1000.0).round() / 1000.0,
            });
        }
    }

    scored.sort_by(|a, b| {
        b.similarity
            .partial_cmp(&a.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(limit);
    Ok(scored)
}

pub fn hybrid_search_history(
    conn: &Connection,
    query: &str,
    query_vector: Option<&[f32]>,
    model: &str,
    limit: usize,
) -> AppResult<Vec<HybridSearchResult>> {
    use std::collections::HashMap;

    let mut fts_ranks: HashMap<i64, usize> = HashMap::new();
    let mut vec_ranks: HashMap<i64, usize> = HashMap::new();
    let mut similarities: HashMap<i64, f64> = HashMap::new();
    let mut url_details: HashMap<i64, (String, String, String, u64, i64)> = HashMap::new();

    let trimmed = query.trim();

    let active_rules = get_active_privacy_rules(conn)?;
    let mut ai_conds = Vec::new();
    let mut ai_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    build_rule_conditions(
        &active_rules,
        DataScope::Ai,
        "u.domain",
        "u.url",
        &mut ai_conds,
        &mut ai_params,
    );
    let ai_clause = if ai_conds.is_empty() {
        String::new()
    } else {
        format!("AND {}", ai_conds.join(" AND "))
    };

    // 1. FTS search (filter AI privacy rules in SQL before LIMIT)
    if !trimmed.is_empty() {
        let cleaned: String = trimmed
            .chars()
            .filter(|c| {
                c.is_alphanumeric() || c.is_whitespace() || *c == '_' || *c == '-' || *c == '.'
            })
            .collect();
        let tokens: Vec<String> = cleaned
            .split_whitespace()
            .filter(|w| !w.is_empty())
            .map(|w| format!("\"{}\"*", w.replace('"', "\"\"")))
            .collect();

        if !tokens.is_empty() {
            let fts_query = tokens.join(" ");
            let fts_sql = format!(
                r#"
                SELECT u.id, u.url, COALESCE(u.title, ''), COALESCE(u.domain, ''),
                       (SELECT count(*) FROM visits v WHERE v.url_id = u.id) as vc,
                       COALESCE((SELECT max(v.visit_time) FROM visits v WHERE v.url_id = u.id), 0) as lvt
                FROM urls_fts fts
                JOIN urls u ON fts.rowid = u.id
                WHERE urls_fts MATCH ?1 {ai_clause}
                ORDER BY rank
                LIMIT 100
                "#
            );
            let mut fts_stmt = conn.prepare(&fts_sql)?;
            let mut fts_params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(1 + ai_params.len());
            fts_params.push(&fts_query);
            for p in &ai_params {
                fts_params.push(p.as_ref());
            }
            let rows = fts_stmt.query_map(fts_params.as_slice(), |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, u64>(4)?,
                    r.get::<_, i64>(5)?,
                ))
            })?;
            let mut valid_rank = 0;
            for row in rows {
                let (id, url, title, domain, vc, lvt) = row?;
                if active_rules
                    .iter()
                    .any(|rule| matches_privacy_rule(&rule.pattern, &domain, &url))
                {
                    continue;
                }
                fts_ranks.insert(id, valid_rank);
                valid_rank += 1;
                url_details.insert(id, (url, title, domain, vc, lvt));
            }
        }
    }

    // 2. Vector search if query_vector is provided (filter AI privacy rules in SQL before LIMIT)
    if let Some(q_vec) = query_vector {
        let vec_sql = format!(
            r#"
            SELECT pe.url_id, u.url, COALESCE(u.title, ''), COALESCE(u.domain, ''),
                   (SELECT count(*) FROM visits v WHERE v.url_id = u.id) as vc,
                   COALESCE((SELECT max(v.visit_time) FROM visits v WHERE v.url_id = u.id), 0) as lvt,
                   pe.embedding
            FROM page_embeddings pe
            JOIN urls u ON pe.url_id = u.id
            WHERE pe.model = ?1 {ai_clause}
            ORDER BY lvt DESC
            LIMIT 4000
            "#
        );
        let mut vec_stmt = conn.prepare(&vec_sql)?;
        let mut vec_params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(1 + ai_params.len());
        vec_params.push(&model);
        for p in &ai_params {
            vec_params.push(p.as_ref());
        }

        let rows = vec_stmt.query_map(vec_params.as_slice(), |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, u64>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, Vec<u8>>(6)?,
            ))
        })?;

        let mut scored = Vec::new();
        for row in rows {
            let (id, url, title, domain, vc, lvt, bytes) = row?;
            if active_rules
                .iter()
                .any(|rule| matches_privacy_rule(&rule.pattern, &domain, &url))
            {
                continue;
            }
            let emb = crate::ai::embedding::bytes_to_floats(&bytes);
            let sim = crate::ai::embedding::cosine_similarity(q_vec, &emb) as f64;
            if sim > 0.25 {
                scored.push((id, url, title, domain, vc, lvt, sim));
            }
        }

        scored.sort_by(|a, b| b.6.partial_cmp(&a.6).unwrap_or(std::cmp::Ordering::Equal));
        for (rank, (id, url, title, domain, vc, lvt, sim)) in
            scored.into_iter().take(100).enumerate()
        {
            vec_ranks.insert(id, rank);
            similarities.insert(id, sim);
            url_details
                .entry(id)
                .or_insert((url, title, domain, vc, lvt));
        }
    }

    // 3. Collect favorites set for ranking boost
    let fav_set: std::collections::HashSet<i64> = {
        if let Ok(mut stmt) = conn.prepare("SELECT url_id FROM favorites") {
            stmt.query_map([], |r| r.get(0))
                .map(|rows| rows.flatten().collect())
                .unwrap_or_default()
        } else {
            std::collections::HashSet::new()
        }
    };

    let now_ms = chrono::Utc::now().timestamp_millis();

    // 4. Compute composite 4-layer scores (FTS + Vector RRF + Recency + Frequency + Favorites)
    let all_ids: Vec<i64> = url_details.keys().cloned().collect();
    let mut results = Vec::new();

    for id in all_ids {
        let fts_r = fts_ranks.get(&id).cloned();
        let vec_r = vec_ranks.get(&id).cloned();
        let sim = similarities.get(&id).cloned().unwrap_or(0.0);
        let base_rrf = crate::ai::embedding::rrf_score(fts_r, vec_r, 60.0);

        if let Some((url, title, domain, vc, lvt)) = url_details.remove(&id) {
            // Recency score (half-life decay over 30 days)
            let days_ago = ((now_ms - lvt).max(0) as f64) / (86400.0 * 1000.0);
            let recency_bonus = 0.002 / (1.0 + days_ago / 30.0);

            // Frequency score (log scale)
            let freq_bonus = ((1.0 + vc as f64).log10() * 0.001).min(0.003);

            // Favorite weight boost
            let fav_bonus = if fav_set.contains(&id) { 0.004 } else { 0.0 };

            let composite_score = base_rrf + recency_bonus + freq_bonus + fav_bonus;

            results.push(HybridSearchResult {
                url_id: id,
                url,
                title,
                domain,
                visit_count: vc,
                last_visit_time: lvt,
                fts_rank: fts_r,
                vec_rank: vec_r,
                similarity_score: (sim * 1000.0).round() / 1000.0,
                rrf_score: (composite_score * 10000.0).round() / 10000.0,
            });
        }
    }

    results.sort_by(|a, b| {
        b.rrf_score
            .partial_cmp(&a.rrf_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(limit);
    Ok(results)
}

// ==========================================
// Milestone E: Topic & Interest Evolution
// ==========================================

pub fn get_all_topics(conn: &Connection) -> AppResult<Vec<TopicItem>> {
    crate::ai::topics::ensure_default_topics(conn)?;
    query_all_topics_readonly(conn)
}

pub fn query_all_topics_readonly(conn: &Connection) -> AppResult<Vec<TopicItem>> {
    let active_rules = get_active_privacy_rules(conn)?;
    let mut hidden_conds = Vec::new();
    let mut hidden_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    build_hidden_rule_conditions(
        &active_rules,
        "u.domain",
        "u.url",
        &mut hidden_conds,
        &mut hidden_params,
    );
    let hidden_clause = if hidden_conds.is_empty() {
        String::new()
    } else {
        format!("AND {}", hidden_conds.join(" AND "))
    };

    // (url_id, topic_id) is the primary key on url_topics, and visits(url_id) is indexed;
    // a correlated scalar subquery avoids multiplying joined rows across full visit history.
    let sql = format!(
        r#"
        SELECT t.id, t.name, t.category, t.icon, t.color,
               COALESCE(SUM(CASE WHEN u.id IS NOT NULL THEN (SELECT COUNT(*) FROM visits v WHERE v.url_id = u.id) ELSE 0 END), 0) as visit_count,
               COUNT(u.id) as url_count
        FROM topics t
        LEFT JOIN url_topics ut ON t.id = ut.topic_id
        LEFT JOIN urls u ON ut.url_id = u.id {}
        GROUP BY t.id
        ORDER BY visit_count DESC
        "#,
        hidden_clause
    );
    let mut stmt = conn.prepare(&sql)?;
    let params_ref: Vec<&dyn rusqlite::ToSql> = hidden_params.iter().map(|b| b.as_ref()).collect();

    let rows = stmt.query_map(params_ref.as_slice(), |r| {
        Ok(TopicItem {
            id: r.get(0)?,
            name: r.get(1)?,
            category: r.get(2)?,
            icon: r.get(3)?,
            color: r.get(4)?,
            visit_count: r.get(5)?,
            url_count: r.get(6)?,
        })
    })?;

    let mut list = Vec::new();
    for r in rows {
        list.push(r?);
    }
    Ok(list)
}

pub fn get_interest_evolution(conn: &Connection, days: u32) -> AppResult<InterestEvolution> {
    interest_evolution_with_scope(conn, days, DataScope::Local)
}

pub fn query_interest_evolution_readonly(
    conn: &Connection,
    days: u32,
) -> AppResult<InterestEvolution> {
    query_interest_evolution_with_scope_readonly(conn, days, DataScope::Local)
}

pub fn get_interest_evolution_for_ai(conn: &Connection, days: u32) -> AppResult<InterestEvolution> {
    interest_evolution_with_scope(conn, days, DataScope::Ai)
}

fn interest_evolution_with_scope(
    conn: &Connection,
    days: u32,
    scope: DataScope,
) -> AppResult<InterestEvolution> {
    crate::ai::topics::ensure_default_topics(conn)?;
    crate::ai::topics::auto_tag_unclassified_urls(conn, 300)?;
    query_interest_evolution_with_scope_readonly(conn, days, scope)
}

fn query_interest_evolution_with_scope_readonly(
    conn: &Connection,
    days: u32,
    scope: DataScope,
) -> AppResult<InterestEvolution> {
    let active_rules = get_active_privacy_rules(conn)?;
    let mut hidden_conds = Vec::new();
    let mut hidden_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    build_rule_conditions(
        &active_rules,
        scope,
        "u.domain",
        "u.url",
        &mut hidden_conds,
        &mut hidden_params,
    );
    let hidden_clause = if hidden_conds.is_empty() {
        String::new()
    } else {
        format!("AND {}", hidden_conds.join(" AND "))
    };

    let now = chrono::Utc::now().timestamp_millis();
    let period_ms = (days as i64) * 86_400_000;
    let curr_start = now - period_ms;
    let prev_start = curr_start - period_ms;

    // Drive from idx_visits_time_id on recent visits >= prev_start so queries touch only
    // the active 2x window instead of scanning every historically tagged URL.
    let sql = format!(
        r#"
        SELECT t.id, t.name, t.category, t.icon, t.color,
               COALESCE(SUM(CASE WHEN v.visit_time >= ? THEN 1 ELSE 0 END), 0) as curr_visits,
               COALESCE(SUM(CASE WHEN v.visit_time >= ? AND v.visit_time < ? THEN 1 ELSE 0 END), 0) as prev_visits
        FROM visits v
        INNER JOIN urls u ON v.url_id = u.id
        INNER JOIN url_topics ut ON u.id = ut.url_id
        INNER JOIN topics t ON ut.topic_id = t.id
        WHERE v.visit_time >= ? {}
        GROUP BY t.id
        HAVING curr_visits > 0 OR prev_visits > 0
        ORDER BY curr_visits DESC
        "#,
        hidden_clause
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut query_params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(4 + hidden_params.len());
    query_params.push(&curr_start);
    query_params.push(&prev_start);
    query_params.push(&curr_start);
    query_params.push(&prev_start);
    for b in &hidden_params {
        query_params.push(b.as_ref());
    }

    let rows = stmt.query_map(query_params.as_slice(), |r| {
        let topic_id: i64 = r.get(0)?;
        let name: String = r.get(1)?;
        let category: String = r.get(2)?;
        let icon: Option<String> = r.get(3)?;
        let color: Option<String> = r.get(4)?;
        let current_visits: u64 = r.get(5)?;
        let previous_visits: u64 = r.get(6)?;

        let growth_rate = if previous_visits == 0 {
            if current_visits > 0 {
                100.0
            } else {
                0.0
            }
        } else {
            let diff = current_visits as f64 - previous_visits as f64;
            ((diff / previous_visits as f64) * 100.0).round()
        };

        Ok(TopicTrend {
            topic_id,
            name,
            category,
            icon,
            color,
            current_visits,
            previous_visits,
            growth_rate,
        })
    })?;

    let mut rising_topics = Vec::new();
    let mut declining_topics = Vec::new();
    let mut new_topics = Vec::new();
    let mut stable_topics = Vec::new();

    for r in rows {
        let trend = r?;
        if trend.previous_visits == 0 && trend.current_visits > 0 {
            new_topics.push(trend);
        } else if trend.growth_rate >= 20.0 {
            rising_topics.push(trend);
        } else if trend.growth_rate <= -20.0 {
            declining_topics.push(trend);
        } else {
            stable_topics.push(trend);
        }
    }

    Ok(InterestEvolution {
        rising_topics,
        declining_topics,
        new_topics,
        stable_topics,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_policy_filters_before_limits_and_counts_the_same_eligible_set() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::database::migrations::run_migrations(&mut conn).unwrap();
        conn.execute_batch("INSERT INTO sources(id,browser,profile,source_type,history_path) VALUES(1,'chrome','Default','sqlite','synthetic');
            INSERT INTO urls(id,url,title,domain) VALUES
            (1,'https://public.example/old','Needle public older','public.example'),
            (2,'https://public.example/recent','Needle public recent','public.example'),
            (3,'https://private.example/secret','Needle private sentinel','private.example'),
            (4,'http://127.0.0.1/internal','Needle loopback sentinel','127.0.0.1'),
            (5,'http://[::1]/internal','Needle IPv6 sentinel','[::1]'),
            (6,'http://localhost/internal','Needle localhost sentinel','localhost'),
            (7,'http://10.1.2.3/internal','Needle LAN sentinel','10.1.2.3'),
            (8,'file:///C:/synthetic/note.txt','Needle file sentinel',''),
            (9,'https://hidden.example/page','Needle hidden sentinel','hidden.example');
            WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<240)
            INSERT INTO visits(source_id,url_id,source_visit_id,visit_time,event_hash,imported_at)
            SELECT 1,CASE WHEN i<3 THEN i WHEN i<120 THEN 2 ELSE 3 END,i,i,'ai-policy-'||i,0 FROM n;").unwrap();
        let private = add_privacy_rule(&conn, "private.example", "private").unwrap();
        add_privacy_rule(&conn, "hidden.example", "hidden").unwrap();
        for query in ["", "Needle", "public.example/"] {
            let citations = query_history_for_ai(&conn, query, 2).unwrap();
            assert_eq!(citations.len(), 2, "query: {query}");
            assert_eq!(citations[0].url, "https://public.example/recent");
            assert_eq!(citations[1].url, "https://public.example/old");
        }
        assert!(query_history_for_ai(&conn, "", 0).unwrap().is_empty());
        let candidates = get_unindexed_urls(&conn, "fixture", 100).unwrap();
        assert_eq!(candidates.len(), 2);
        assert!(candidates
            .iter()
            .all(|(_, url, _, _)| url.starts_with("https://public.example/")));
        save_page_embeddings(
            &conn,
            &[
                (1, "public".into(), vec![1.0, 0.0], "fixture"),
                (3, "old private".into(), vec![0.0, 1.0], "fixture"),
            ],
        )
        .unwrap();
        let status = get_embedding_indexing_status(&conn, "fixture").unwrap();
        assert_eq!(
            (status.total_urls, status.indexed_urls, status.pending_urls),
            (2, 1, 1)
        );
        toggle_privacy_rule(&conn, private.id, false).unwrap();
        let status = get_embedding_indexing_status(&conn, "fixture").unwrap();
        assert_eq!(
            (status.total_urls, status.indexed_urls, status.pending_urls),
            (3, 2, 1)
        );
        conn.execute(
            "UPDATE privacy_rules SET enabled=0 WHERE pattern='ip:non-public'",
            [],
        )
        .unwrap();
        let candidates = get_unindexed_urls(&conn, "fixture", 100).unwrap();
        assert!(candidates
            .iter()
            .any(|(_, url, _, _)| url.contains("[::1]")));
        // Older literal rules remain effective independently of the new token.
        assert!(candidates
            .iter()
            .all(|(_, url, _, _)| !url.contains("127.0.0.1")));
        assert!(candidates
            .iter()
            .all(|(_, url, _, _)| !url.starts_with("file:")));
        let empty = get_embedding_indexing_status(&conn, "other-model").unwrap();
        assert_eq!(empty.indexed_urls, 0);
    }

    #[test]
    fn ai_interest_scope_excludes_private_contributions_without_hiding_local_history() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::database::migrations::run_migrations(&mut conn).unwrap();
        conn.execute_batch("INSERT INTO sources(id,browser,profile,source_type,history_path) VALUES(1,'chrome','Default','sqlite','synthetic');
            INSERT INTO urls(id,url,title,domain) VALUES
            (1,'https://public.example/page','Public','public.example'),
            (2,'https://private.example/page','Secret','private.example'),
            (3,'http://127.0.0.1/page','Internal','127.0.0.1'),
            (4,'file:///C:/synthetic/note.txt','Local file','');
            INSERT INTO topics(id,name,category,created_at) VALUES(1001,'Synthetic mixed','fixture',0),(1002,'Synthetic private only','fixture',0),(1003,'Synthetic internal only','fixture',0),(1004,'Synthetic file only','fixture',0);
            INSERT INTO url_topics(url_id,topic_id) VALUES(1,1001),(2,1001),(2,1002),(3,1003),(4,1004);").unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        for id in 1..=4 {
            conn.execute("INSERT INTO visits(source_id,url_id,source_visit_id,visit_time,event_hash,imported_at) VALUES(1,?1,?1,?2,?3,0)",params![id,now,format!("ai-evolution-{id}")]).unwrap();
        }
        add_privacy_rule(&conn, "private.example", "private").unwrap();
        let local = get_interest_evolution(&conn, 30).unwrap();
        let ai = get_interest_evolution_for_ai(&conn, 30).unwrap();
        let local_topics: Vec<_> = local
            .new_topics
            .iter()
            .chain(&local.rising_topics)
            .chain(&local.stable_topics)
            .chain(&local.declining_topics)
            .collect();
        let ai_topics: Vec<_> = ai
            .new_topics
            .iter()
            .chain(&ai.rising_topics)
            .chain(&ai.stable_topics)
            .chain(&ai.declining_topics)
            .collect();
        assert_eq!(
            local_topics
                .iter()
                .find(|t| t.topic_id == 1001)
                .unwrap()
                .current_visits,
            2
        );
        assert!(local_topics.iter().any(|t| t.topic_id == 1002));
        assert_eq!(
            ai_topics
                .iter()
                .find(|t| t.topic_id == 1001)
                .unwrap()
                .current_visits,
            1
        );
        assert!(ai_topics.iter().all(|t| !matches!(t.topic_id, 1002..=1004)));
    }

    #[test]
    fn builtin_network_rule_is_persisted_toggleable_and_consistent_with_sql() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::database::migrations::run_migrations(&mut conn).unwrap();
        conn.execute_batch("INSERT INTO sources(id,browser,profile,source_type,history_path) VALUES(1,'chrome','Synthetic','sqlite','fixture');INSERT INTO urls(id,url,title,domain) VALUES(1,'http://192.168.2.1/wiki','LAN','192.168.2.1'),(2,'http://[fd00::1]/wiki','LAN v6','fd00::1'),(3,'https://public-fixture.test/wiki','Public','public-fixture.test');INSERT INTO visits(id,source_id,url_id,source_visit_id,visit_time,visit_duration,event_hash,imported_at) VALUES(1,1,1,1,1000,0,'network-1',0),(2,1,2,2,2000,0,'network-2',0),(3,1,3,3,3000,0,'network-3',0);").unwrap();
        assert_eq!(
            query_history(&conn, &HistoryFilter::default())
                .unwrap()
                .items
                .len(),
            3
        );
        let ai = query_history_for_ai(&conn, "", 10).unwrap();
        assert_eq!(ai.len(), 1);
        assert_eq!(ai[0].domain, "public-fixture.test");
        conn.execute(
            "UPDATE privacy_rules SET rule_type='hidden' WHERE pattern='ip:non-public'",
            [],
        )
        .unwrap();
        assert_eq!(
            query_history(&conn, &HistoryFilter::default())
                .unwrap()
                .items
                .len(),
            1
        );
        assert_eq!(
            get_analytics_summary(&conn, None, None, None)
                .unwrap()
                .total_visits,
            1
        );
        conn.execute(
            "UPDATE privacy_rules SET enabled=0 WHERE pattern='ip:non-public'",
            [],
        )
        .unwrap();
        assert_eq!(query_history_for_ai(&conn, "", 10).unwrap().len(), 3);
        assert!(matches_privacy_rule(
            "ip:non-public",
            "",
            "http://[::ffff:10.1.2.3]/"
        ));
        assert!(!matches_privacy_rule(
            "ip:non-public",
            "",
            "https://localhost.evil.test/"
        ));
    }

    #[test]
    fn multi_chunk_delete_rolls_back_on_late_failure_and_cancellation() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::database::migrations::run_migrations(&mut conn).unwrap();
        conn.execute_batch("INSERT INTO sources(id,browser,profile,source_type,history_path) VALUES(1,'chrome','Synthetic','sqlite','fixture');INSERT INTO urls(id,url,title,domain) VALUES(1,'https://delete-fixture.test/','Synthetic','delete-fixture.test');WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<1001) INSERT INTO visits(id,source_id,url_id,source_visit_id,visit_time,visit_duration,event_hash,imported_at) SELECT i,1,1,i,i,0,'delete-fixture-'||i,0 FROM n;CREATE TRIGGER fail_late_delete BEFORE DELETE ON visits WHEN OLD.id=501 BEGIN SELECT RAISE(ABORT,'synthetic delete failure'); END;").unwrap();
        let ids: Vec<i64> = (1..=1001).collect();
        assert!(delete_visits(&conn, &ids).is_err());
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM visits", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1001
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM visit_exclusions", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0,
            "a failed delete transaction must not leave exclusion rows"
        );
        conn.execute_batch("DROP TRIGGER fail_late_delete;")
            .unwrap();
        let checks = std::cell::Cell::new(0);
        let result = delete_visits_checked(&conn, &ids, || {
            checks.set(checks.get() + 1);
            if checks.get() >= 3 {
                Err(AppError::VaultLocked("synthetic cancellation".into()))
            } else {
                Ok(())
            }
        });
        assert!(result.is_err());
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM visits", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1001
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM visit_exclusions", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0,
            "cancellation must roll back inserted exclusions"
        );
        assert_eq!(delete_visits(&conn, &ids).unwrap(), 1001);
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM visit_exclusions", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1001
        );
    }

    #[test]
    fn sessions_include_later_sources_after_ten_thousand_rows_and_keep_recent_limit() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::database::migrations::run_migrations(&mut conn).unwrap();
        conn.execute_batch("INSERT INTO sources(id,browser,profile,source_type,history_path) VALUES(1,'chrome','First','sqlite','fixture'),(2,'edge','Later','sqlite','fixture');INSERT INTO urls(id,url,title,domain) VALUES(1,'https://sessions-fixture.test/','Synthetic','sessions-fixture.test');WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<10003) INSERT INTO visits(id,source_id,url_id,source_visit_id,visit_time,visit_duration,event_hash,imported_at) SELECT i,1,1,i,i*1000,0,'sessions-fixture-'||i,0 FROM n;INSERT INTO visits(id,source_id,url_id,source_visit_id,visit_time,visit_duration,event_hash,imported_at) VALUES(10004,2,1,1,20000000,0,'later-fixture',0);").unwrap();
        let all = get_research_sessions(&conn, None, 10).unwrap();
        assert_eq!(all.iter().map(|s| s.visit_count as u64).sum::<u64>(), 10004);
        let latest = get_research_sessions(&conn, None, 1).unwrap();
        assert_eq!(latest.len(), 1);
        assert_eq!(latest[0].profile, "Later");
        add_privacy_rule(&conn, "sessions-fixture.test", "private").unwrap();
        assert_eq!(get_research_sessions(&conn, None, 10).unwrap().len(), 2);
        assert!(get_research_sessions_for_ai(&conn, None, 10)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn analytics_preserves_local_calendar_privacy_and_window_semantics() {
        use chrono::{Datelike, TimeZone, Timelike};
        let mut conn = Connection::open_in_memory().unwrap();
        crate::database::migrations::run_migrations(&mut conn).unwrap();
        conn.execute_batch("INSERT INTO sources(id,browser,profile,source_type,history_path) VALUES(1,'chrome','Default','sqlite','synthetic'),(2,'firefox','Default','sqlite','synthetic');
            INSERT INTO urls(id,url,title,domain) VALUES
            (1,'https://allowed.test/news','Allowed','allowed.test'),
            (2,'https://private.test/news','Private','private.test'),
            (3,'https://allowed.test/hidden','Hidden URL','allowed.test'),
            (4,'https://hidden.test/news','Hidden domain','hidden.test'),
            (5,'https://missing-domain.test/','Null domain',NULL),
            (6,'https://empty-domain.test/','Empty domain','');").unwrap();
        add_privacy_rule(&conn, "allowed.test/hidden", "hidden").unwrap();
        add_privacy_rule(&conn, "hidden.test", "hidden").unwrap();
        add_privacy_rule(&conn, "private.test", "private").unwrap();
        let start = 1_700_000_000_000i64;
        let hour = 3_600_000;
        let end = start + 24 * hour;
        let visits = [
            (1, start - 1000),
            (1, start),
            (1, start + hour + 123),
            (1, end),
            (2, start + 2 * hour),
            (2, start + 26 * hour),
            (2, start + 27 * hour),
            (3, start - 500),
            (3, start + 5 * hour),
            (3, start + 6 * hour),
            (4, start + 7 * hour),
            (4, start + 8 * hour),
            (4, start + 9 * hour),
            (5, start + 3 * hour),
            (6, start + 4 * hour),
        ];
        let mut expected_days = std::collections::BTreeMap::<String, u64>::new();
        let mut expected_hours = [0u64; 24];
        let mut expected_week = [0u64; 168];
        for (index, (url, time)) in visits.into_iter().enumerate() {
            let source = if url == 2 || url == 6 { 2 } else { 1 };
            conn.execute("INSERT INTO visits(id,source_id,url_id,source_visit_id,visit_time,event_hash,imported_at) VALUES(?1,?2,?3,?1,?4,?5,?4)",params![index as i64+1,source,url,time,format!("analytics-{index}")]).unwrap();
            if ![3, 4].contains(&url) && time >= start && time <= end {
                let local = chrono::Local.timestamp_millis_opt(time).single().unwrap();
                *expected_days
                    .entry(local.format("%Y-%m-%d").to_string())
                    .or_default() += 1;
                expected_hours[local.hour() as usize] += 1;
                expected_week[local.weekday().num_days_from_sunday() as usize * 24
                    + local.hour() as usize] += 1;
            }
        }
        let actual = get_analytics_summary(&conn, Some(start), Some(end), Some(1)).unwrap();
        assert_eq!(
            (
                actual.total_visits,
                actual.unique_urls,
                actual.unique_domains
            ),
            (6, 4, 2)
        );
        assert_eq!(actual.active_days, expected_days.len() as u64);
        assert_eq!(actual.revisit_rate, 33.3);
        assert_eq!(
            (
                actual.period_comparison.previous_count,
                actual.period_comparison.growth_rate
            ),
            (1, 500.0)
        );
        assert_eq!(
            actual
                .daily_trend
                .iter()
                .map(|d| (d.date.clone(), d.count))
                .collect::<std::collections::BTreeMap<_, _>>(),
            expected_days
        );
        assert_eq!(
            actual
                .hourly_distribution
                .iter()
                .map(|h| h.count)
                .collect::<Vec<_>>(),
            expected_hours.to_vec()
        );
        assert_eq!(
            actual
                .weekly_heatmap
                .iter()
                .map(|h| h.count)
                .collect::<Vec<_>>(),
            expected_week.to_vec()
        );
        assert_eq!(
            actual
                .forgotten_gems
                .iter()
                .map(|g| (g.url_id, g.total_visits))
                .collect::<Vec<_>>(),
            vec![(1, 4), (2, 3)]
        );
        assert_eq!(
            actual
                .browser_breakdown
                .iter()
                .map(|b| (b.name.as_str(), b.count, b.percent))
                .collect::<Vec<_>>(),
            vec![("chrome", 4, 67), ("firefox", 2, 33)]
        );
        assert!(!actual
            .domain_ranking
            .iter()
            .any(|d| d.domain == "hidden.test"));
        let empty = get_analytics_summary(
            &conn,
            Some(start + 60 * hour),
            Some(start + 61 * hour),
            None,
        )
        .unwrap();
        assert_eq!(
            (
                empty.total_visits,
                empty.unique_urls,
                empty.unique_domains,
                empty.active_days
            ),
            (0, 0, 0, 0)
        );
        assert!(empty.daily_trend.is_empty());
        assert!(empty.hourly_distribution.iter().all(|h| h.count == 0));
        assert_eq!(
            empty.forgotten_gems.len(),
            2,
            "forgotten pages retain their existing all-history scope"
        );
        conn.execute("UPDATE visits SET visit_time=?1 WHERE id=2", [i64::MAX])
            .unwrap();
        assert!(
            get_analytics_summary(&conn, Some(0), Some(i64::MAX), None).is_err(),
            "invalid calendar values must not silently disappear"
        );
    }

    #[test]
    fn test_extract_search_query_engines() {
        let google_url = "https://www.google.com/search?q=rust+tauri+v2&oq=rust";
        let res = extract_search_query(google_url);
        assert_eq!(
            res,
            Some(("Google".to_string(), "rust tauri v2".to_string()))
        );

        let baidu_url =
            "https://www.baidu.com/s?wd=%E6%B5%8F%E8%A7%88%E5%99%A8%E5%8E%86%E5%8F%B2&rsv_spt=1";
        let res = extract_search_query(baidu_url);
        assert_eq!(res, Some(("Baidu".to_string(), "浏览器历史".to_string())));

        let bing_url = "https://cn.bing.com/search?q=Antigravity+AI";
        let res = extract_search_query(bing_url);
        assert_eq!(
            res,
            Some(("Bing".to_string(), "Antigravity AI".to_string()))
        );

        let github_url = "https://github.com/search?q=repo%3Arust-lang%2Frust+compiler";
        let res = extract_search_query(github_url);
        assert_eq!(
            res,
            Some((
                "GitHub".to_string(),
                "repo:rust-lang/rust compiler".to_string()
            ))
        );

        let bilibili_url = "https://search.bilibili.com/all?keyword=%E9%95%87%E9%AD%82%E8%A1%97";
        let res = extract_search_query(bilibili_url);
        assert_eq!(res, Some(("Bilibili".to_string(), "镇魂街".to_string())));

        let non_search = "https://docs.rs/rusqlite/latest/rusqlite/";
        assert_eq!(extract_search_query(non_search), None);
    }

    #[test]
    fn test_query_history_for_ai_privacy_filter() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::database::migrations::run_migrations(&mut conn).unwrap();

        // 1. Insert source
        conn.execute(
            "INSERT INTO sources (id, browser, profile, source_type, history_path) VALUES (1, 'chrome', 'Default', 'sqlite', '/tmp/hist')",
            [],
        ).unwrap();

        // 2. Insert URLs & Visits
        conn.execute(
            "INSERT INTO urls (id, url, title, domain) VALUES (1, 'https://docs.rs/tokio', 'Tokio Async Rust', 'docs.rs')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO urls (id, url, title, domain) VALUES (2, 'https://secret-bank.internal/account', 'Banking Secret Account', 'secret-bank.internal')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO urls (id, url, title, domain) VALUES (3, 'https://github.com/rust-lang/rust', 'Rust Programming Language', 'github.com')",
            [],
        ).unwrap();

        let now = chrono::Utc::now().timestamp_millis();
        conn.execute(
            "INSERT INTO visits (source_id, url_id, source_visit_id, visit_time, event_hash, imported_at) VALUES (1, 1, 101, ?1, 'hash1', ?1)",
            rusqlite::params![now - 3000],
        ).unwrap();
        conn.execute(
            "INSERT INTO visits (source_id, url_id, source_visit_id, visit_time, event_hash, imported_at) VALUES (1, 2, 102, ?1, 'hash2', ?1)",
            rusqlite::params![now - 2000],
        ).unwrap();
        conn.execute(
            "INSERT INTO visits (source_id, url_id, source_visit_id, visit_time, event_hash, imported_at) VALUES (1, 3, 103, ?1, 'hash3', ?1)",
            rusqlite::params![now - 1000],
        ).unwrap();

        // 3. Add privacy rule to block secret-bank.internal
        conn.execute(
            "INSERT INTO privacy_rules (pattern, rule_type, enabled, created_at) VALUES ('secret-bank.internal', 'block', 1, ?1)",
            rusqlite::params![now],
        ).unwrap();

        // 4. Query AI for "rust" - should return both docs.rs and github.com
        let citations_rust = query_history_for_ai(&conn, "rust", 10).unwrap();
        let urls_rust: Vec<String> = citations_rust.into_iter().map(|c| c.url).collect();
        assert_eq!(urls_rust.len(), 2);
        assert!(urls_rust.contains(&"https://docs.rs/tokio".to_string()));
        assert!(urls_rust.contains(&"https://github.com/rust-lang/rust".to_string()));

        // 5. Query AI for "Banking" or "secret" - should be BLOCKED and excluded completely
        let citations_secret = query_history_for_ai(&conn, "Banking", 10).unwrap();
        assert_eq!(citations_secret.len(), 0);
    }

    #[test]
    fn test_milestone_c_profile_and_sorting() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::database::migrations::run_migrations(&mut conn).unwrap();

        conn.execute(
            "INSERT INTO sources (id, browser, profile, source_type, history_path) VALUES (1, 'chrome', 'WorkProfile', 'sqlite', '/tmp/p1')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO sources (id, browser, profile, source_type, history_path) VALUES (2, 'chrome', 'Personal', 'sqlite', '/tmp/p2')",
            [],
        ).unwrap();

        conn.execute("INSERT INTO urls (id, url, title, domain) VALUES (1, 'https://work.com/doc1', 'Work Doc', 'work.com')", []).unwrap();
        conn.execute("INSERT INTO urls (id, url, title, domain) VALUES (2, 'https://game.com/play', 'Fun Game', 'game.com')", []).unwrap();

        let t1 = 1700000000000i64;
        let t2 = 1700000010000i64;
        conn.execute(
            "INSERT INTO visits (source_id, url_id, source_visit_id, visit_time, visit_duration, event_hash, imported_at) VALUES (1, 1, 1, ?1, 10, 'h1', ?1)",
            params![t1],
        ).unwrap();
        conn.execute(
            "INSERT INTO visits (source_id, url_id, source_visit_id, visit_time, visit_duration, event_hash, imported_at) VALUES (2, 2, 2, ?1, 50, 'h2', ?1)",
            params![t2],
        ).unwrap();

        // 1. Test profile filter
        let filter_work = HistoryFilter {
            profiles: Some(vec!["WorkProfile".to_string()]),
            ..Default::default()
        };
        let res = query_history(&conn, &filter_work).unwrap();
        assert_eq!(res.items.len(), 1);
        assert_eq!(res.items[0].profile, "WorkProfile");

        // 2. Test profile: AST query
        let filter_ast = HistoryFilter {
            search: Some("profile:personal".to_string()),
            ..Default::default()
        };
        let res_ast = query_history(&conn, &filter_ast).unwrap();
        assert_eq!(res_ast.items.len(), 1);
        assert_eq!(res_ast.items[0].profile, "Personal");

        // 3. Test sorting by duration
        let filter_sort_dur = HistoryFilter {
            sort_by: Some("duration".to_string()),
            ..Default::default()
        };
        let res_sort = query_history(&conn, &filter_sort_dur).unwrap();
        assert_eq!(res_sort.items.len(), 2);
        assert_eq!(res_sort.items[0].visit_duration, 50);
        assert_eq!(res_sort.items[1].visit_duration, 10);
    }

    #[test]
    fn test_smart_collections_crud() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::database::migrations::run_migrations(&mut conn).unwrap();

        let created = create_smart_collection(
            &conn,
            "GitHub Issues",
            "site:github.com issues",
            Some(r#"{"tag": "work"}"#),
        )
        .unwrap();
        assert_eq!(created.name, "GitHub Issues");

        let list = list_smart_collections(&conn).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, created.id);

        delete_smart_collection(&conn, created.id).unwrap();
        let list_after = list_smart_collections(&conn).unwrap();
        assert_eq!(list_after.len(), 0);
    }

    #[test]
    fn test_domain_ranking_and_detail() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::database::migrations::run_migrations(&mut conn).unwrap();

        conn.execute("INSERT INTO sources (id, browser, profile, source_type, history_path) VALUES (1, 'chrome', 'Default', 'sqlite', '/tmp')", []).unwrap();
        conn.execute("INSERT INTO urls (id, url, title, domain) VALUES (1, 'https://github.com/rust-lang/rust', 'Rust', 'github.com')", []).unwrap();
        conn.execute("INSERT INTO urls (id, url, title, domain) VALUES (2, 'https://github.com/tauri-apps/tauri', 'Tauri', 'github.com')", []).unwrap();

        let now = chrono::Utc::now().timestamp_millis();
        conn.execute("INSERT INTO visits (source_id, url_id, source_visit_id, visit_time, event_hash, imported_at) VALUES (1, 1, 1, ?1, 'v1', ?1)", params![now - 2000]).unwrap();
        conn.execute("INSERT INTO visits (source_id, url_id, source_visit_id, visit_time, event_hash, imported_at) VALUES (1, 2, 2, ?1, 'v2', ?1)", params![now - 1000]).unwrap();
        conn.execute("INSERT INTO visits (source_id, url_id, source_visit_id, visit_time, event_hash, imported_at) VALUES (1, 1, 3, ?1, 'v3', ?1)", params![now]).unwrap();

        let ranking = get_website_ranking(&conn, None, None, Some(10)).unwrap();
        assert_eq!(ranking.len(), 1);
        assert_eq!(ranking[0].domain, "github.com");
        assert_eq!(ranking[0].visits, 3);
        assert_eq!(ranking[0].unique_urls, 2);

        let detail = get_domain_detail(&conn, "github.com", None, None).unwrap();
        assert_eq!(detail.total_visits, 3);
        assert_eq!(detail.unique_urls, 2);
        assert!(!detail.path_tree.is_empty());
        assert_eq!(detail.path_tree[0].name, "rust-lang");
    }

    #[test]
    fn test_query_history_and_indexing_privacy_filters() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::database::migrations::run_migrations(&mut conn).unwrap();

        conn.execute(
            "INSERT INTO sources (id, browser, profile, source_type, history_path) VALUES (1, 'chrome', 'Default', 'sqlite', '/tmp')",
            [],
        ).unwrap();

        // 1. Insert 3 URLs: public, hidden, private
        conn.execute("INSERT INTO urls (id, url, title, domain) VALUES (1, 'https://public.org/news', 'Public News', 'public.org')", []).unwrap();
        conn.execute("INSERT INTO urls (id, url, title, domain) VALUES (2, 'https://secret.corp/admin', 'Internal Admin', 'secret.corp')", []).unwrap();
        conn.execute("INSERT INTO urls (id, url, title, domain) VALUES (3, 'https://banking.net/my-card', 'My Banking', 'banking.net')", []).unwrap();

        let now = chrono::Utc::now().timestamp_millis();
        conn.execute("INSERT INTO visits (source_id, url_id, source_visit_id, visit_time, event_hash, imported_at) VALUES (1, 1, 1, ?1, 'v1', ?1)", params![now - 2000]).unwrap();
        conn.execute("INSERT INTO visits (source_id, url_id, source_visit_id, visit_time, event_hash, imported_at) VALUES (1, 2, 2, ?1, 'v2', ?1)", params![now - 1000]).unwrap();
        conn.execute("INSERT INTO visits (source_id, url_id, source_visit_id, visit_time, event_hash, imported_at) VALUES (1, 3, 3, ?1, 'v3', ?1)", params![now]).unwrap();

        // 2. Add hidden rule for secret.corp and private rule for banking.net
        conn.execute("INSERT INTO privacy_rules (pattern, rule_type, enabled, created_at) VALUES ('secret.corp', 'hidden', 1, ?1)", params![now]).unwrap();
        conn.execute("INSERT INTO privacy_rules (pattern, rule_type, enabled, created_at) VALUES ('banking.net', 'private', 1, ?1)", params![now]).unwrap();

        // 3. Test query_history: hidden rule ('secret.corp') must be excluded; private rule ('banking.net') and public ('public.org') remain
        let page = query_history(&conn, &HistoryFilter::default()).unwrap();
        let page_domains: Vec<String> = page.items.into_iter().map(|i| i.domain).collect();
        assert_eq!(page_domains.len(), 2);
        assert!(!page_domains.contains(&"secret.corp".to_string()));
        assert!(page_domains.contains(&"public.org".to_string()));
        assert!(page_domains.contains(&"banking.net".to_string()));

        // 4. Test get_unindexed_urls: BOTH hidden ('secret.corp') and private ('banking.net') must be excluded from AI indexing!
        let unindexed = get_unindexed_urls(&conn, "text-embedding-3-small", 10).unwrap();
        let unindexed_urls: Vec<String> = unindexed.into_iter().map(|(_, u, _, _)| u).collect();
        assert_eq!(unindexed_urls.len(), 1);
        assert_eq!(unindexed_urls[0], "https://public.org/news");

        // 5. Test get_analytics_summary: hidden ('secret.corp') must NOT appear in domain ranking or stats!
        let analytics = get_analytics_summary(&conn, None, None, None).unwrap();
        assert_eq!(analytics.total_visits, 2);
        assert!(!analytics
            .domain_ranking
            .iter()
            .any(|d| d.domain == "secret.corp"));

        // 6. Test get_domain_detail on hidden domain: should return zeroed stats
        let hidden_detail = get_domain_detail(&conn, "secret.corp", None, None).unwrap();
        assert_eq!(hidden_detail.total_visits, 0);
        assert_eq!(hidden_detail.unique_urls, 0);

        // 7. Test get_on_this_day: hidden domain must be excluded
        let otd = get_on_this_day(&conn, None, None, None, None).unwrap();
        assert!(!otd.items.iter().any(|i| i.domain == "secret.corp"));

        // 8. Test get_all_topics & get_interest_evolution: hidden domain excluded
        let topics = get_all_topics(&conn).unwrap();
        assert!(!topics.iter().any(|t| t.name == "secret.corp"));

        // 9. Local research sessions may include Private history, but the
        // cloud-AI variant must exclude both Private and Hidden records.
        let local_sessions = get_research_sessions(&conn, None, 100).unwrap();
        assert_eq!(
            local_sessions
                .iter()
                .map(|session| session.visit_count as u64)
                .sum::<u64>(),
            2
        );
        let ai_sessions = get_research_sessions_for_ai(&conn, None, 100).unwrap();
        assert_eq!(
            ai_sessions
                .iter()
                .map(|session| session.visit_count as u64)
                .sum::<u64>(),
            1
        );

        // 10. Test matches_privacy_rule logic
        assert!(matches_privacy_rule(
            "*.internal.com",
            "sub.internal.com",
            "https://sub.internal.com/1"
        ));
        assert!(matches_privacy_rule(
            "*.internal.com",
            "internal.com",
            "https://internal.com/1"
        ));
        assert!(!matches_privacy_rule(
            "*.internal.com",
            "other.com",
            "https://other.com/1"
        ));
        assert!(!matches_privacy_rule(
            "*.internal.com",
            "evilinternal.com",
            "https://evilinternal.com/1"
        ));
        assert!(matches_privacy_rule(
            "token=secret",
            "any.com",
            "https://any.com?token=secret"
        ));
    }

    #[test]
    fn test_url_specific_hidden_rules_bound_detail_aggregates_and_topics() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::database::migrations::run_migrations(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO sources (id, browser, profile, source_type, history_path) VALUES (1, 'chrome', 'Default', 'sqlite', '/tmp')",
            [],
        )
        .unwrap();

        let urls = [
            (1, "https://example.com/public", "Public", "example.com"),
            (2, "https://example.com/private", "Hidden", "example.com"),
            (
                3,
                "https://new.example/visible",
                "New visible",
                "new.example",
            ),
            (
                4,
                "https://new.example/private",
                "New hidden",
                "new.example",
            ),
            (
                5,
                "https://dormant.example/visible",
                "Dormant visible",
                "dormant.example",
            ),
            (
                6,
                "https://dormant.example/private",
                "Dormant hidden",
                "dormant.example",
            ),
        ];
        for (id, url, title, domain) in urls {
            conn.execute(
                "INSERT INTO urls (id, url, title, domain) VALUES (?1, ?2, ?3, ?4)",
                params![id, url, title, domain],
            )
            .unwrap();
        }

        let now = chrono::Utc::now().timestamp_millis();
        let window_start = now - 100_000;
        let window_end = now + 100_000;
        let mut visit_no = 1i64;
        let mut add_visit = |url_id: i64, visit_time: i64| {
            let event_hash = format!("url-specific-{visit_no}");
            conn.execute(
                "INSERT INTO visits (source_id, url_id, source_visit_id, visit_time, event_hash, imported_at) VALUES (1, ?1, ?2, ?3, ?4, ?3)",
                params![url_id, visit_no, visit_time, event_hash],
            )
            .unwrap();
            visit_no += 1;
        };

        // The example domain has two public visits and three URL-specific
        // hidden visits. Hidden visits are deliberately interleaved so that
        // nearby and top/path LIMIT behavior is exercised.
        add_visit(1, now - 1_000);
        add_visit(2, now - 800);
        add_visit(2, now - 600);
        add_visit(1, now - 400);
        add_visit(2, now - 200);

        // The hidden URL is the apparent first visit; the domain should still
        // be reported as new based on the first visible visit.
        add_visit(4, window_start + 10);
        add_visit(3, window_start + 20);

        // The hidden URL supplies historical noise, but only five visible
        // visits should satisfy the dormant threshold.
        for i in 1..=5 {
            add_visit(5, window_start - i * 100);
        }
        for i in 1..=8 {
            add_visit(6, window_start - 10_000 - i * 100);
        }

        conn.execute(
            "INSERT INTO privacy_rules (pattern, rule_type, enabled, created_at) VALUES (?1, 'hidden', 1, ?2)",
            params!["https://example.com/private", now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO privacy_rules (pattern, rule_type, enabled, created_at) VALUES (?1, 'hidden', 1, ?2)",
            params!["https://new.example/private", now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO privacy_rules (pattern, rule_type, enabled, created_at) VALUES (?1, 'hidden', 1, ?2)",
            params!["https://dormant.example/private", now],
        )
        .unwrap();

        // A hidden current visit must not be disclosed, and nearby results
        // must contain four visible neighbors at most rather than four raw
        // rows followed by a lossy post-filter.
        let hidden_visit_id: i64 = conn
            .query_row(
                "SELECT id FROM visits WHERE url_id = 2 ORDER BY visit_time LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(get_visit_detail(&conn, hidden_visit_id).unwrap().is_none());
        let public_visit_id: i64 = conn
            .query_row(
                "SELECT id FROM visits WHERE url_id = 1 ORDER BY visit_time DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let detail = get_visit_detail(&conn, public_visit_id).unwrap().unwrap();
        assert!(detail
            .nearby_visits
            .iter()
            .all(|item| item.url != "https://example.com/private"));
        assert_eq!(
            detail
                .nearby_visits
                .iter()
                .filter(|item| item.url == "https://example.com/public")
                .count(),
            2
        );

        let ranking =
            get_website_ranking(&conn, Some(window_start), Some(window_end), Some(100)).unwrap();
        let example_ranking = ranking
            .iter()
            .find(|item| item.domain == "example.com")
            .unwrap();
        assert_eq!(example_ranking.visits, 2);
        assert_eq!(example_ranking.unique_urls, 1);

        let domain_detail =
            get_domain_detail(&conn, "example.com", Some(window_start), Some(window_end)).unwrap();
        assert_eq!(domain_detail.total_visits, 2);
        assert_eq!(domain_detail.unique_urls, 1);
        assert_eq!(domain_detail.top_urls.len(), 1);
        assert_eq!(domain_detail.top_urls[0].url, "https://example.com/public");
        assert!(domain_detail
            .path_tree
            .iter()
            .all(|node| node.full_path != "/private"));
        assert_eq!(
            domain_detail
                .hourly_distribution
                .iter()
                .map(|item| item.count)
                .sum::<u64>(),
            2
        );

        let dynamics = get_domain_dynamics(&conn, Some(window_start), Some(window_end)).unwrap();
        let new_item = dynamics
            .new_domains
            .iter()
            .find(|item| item.domain == "new.example")
            .unwrap();
        assert_eq!(new_item.visits_in_period, 1);
        assert_eq!(new_item.sample_url, "https://new.example/visible");
        let dormant_item = dynamics
            .dormant_domains
            .iter()
            .find(|item| item.domain == "dormant.example")
            .unwrap();
        assert_eq!(dormant_item.total_historical_visits, 5);

        let sessions = get_research_sessions(&conn, None, 100).unwrap();
        assert_eq!(
            sessions
                .iter()
                .map(|session| session.visit_count as u64)
                .sum::<u64>(),
            8
        );

        conn.execute(
            "INSERT INTO topics (name, category, created_at) VALUES ('URL-specific topic', 'test', ?1)",
            params![now],
        )
        .unwrap();
        let topic_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO url_topics (url_id, topic_id) VALUES (1, ?1)",
            params![topic_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO url_topics (url_id, topic_id) VALUES (2, ?1)",
            params![topic_id],
        )
        .unwrap();
        let topic = get_all_topics(&conn)
            .unwrap()
            .into_iter()
            .find(|item| item.id == topic_id)
            .unwrap();
        assert_eq!(topic.url_count, 1);
        assert_eq!(topic.visit_count, 2);

        let evolution = get_interest_evolution(&conn, 30).unwrap();
        let trend = evolution
            .new_topics
            .iter()
            .chain(evolution.rising_topics.iter())
            .chain(evolution.stable_topics.iter())
            .find(|item| item.topic_id == topic_id)
            .unwrap();
        assert_eq!(trend.current_visits, 2);
        assert_eq!(trend.previous_visits, 0);
    }

    #[test]
    fn test_null_domain_does_not_drop_unmatched_ai_index_candidates() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::database::migrations::run_migrations(&mut conn).unwrap();

        conn.execute(
            "INSERT INTO urls (id, url, title, domain) VALUES (1, 'https://public.example/page', 'Public', NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO privacy_rules (pattern, rule_type, enabled, created_at) VALUES ('secret.example', 'hidden', 1, 1)",
            [],
        )
        .unwrap();

        let candidates = get_unindexed_urls(&conn, "test-model", 10).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].1, "https://public.example/page");

        let status = get_embedding_indexing_status(&conn, "test-model").unwrap();
        assert_eq!(status.total_urls, 1);
        assert_eq!(status.pending_urls, 1);
    }

    #[test]
    fn test_privacy_rule_types_and_like_patterns_are_fail_closed() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::database::migrations::run_migrations(&mut conn).unwrap();

        let rule = add_privacy_rule(&conn, " foo%bar ", "HIDDEN").unwrap();
        assert_eq!(rule.pattern, "foo%bar");
        assert_eq!(rule.rule_type, "hidden");
        assert!(add_privacy_rule(&conn, "invalid", "other").is_err());
        assert!(add_privacy_rule(&conn, "", "hidden").is_err());

        conn.execute(
            "INSERT INTO urls (id, url, title, domain) VALUES (1, 'https://fooXbar.example/page', 'Not a match', NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO urls (id, url, title, domain) VALUES (2, 'https://foo%bar.example/page', 'Literal match', NULL)",
            [],
        )
        .unwrap();

        let candidates = get_unindexed_urls(&conn, "test-model", 10).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].1, "https://fooXbar.example/page");
    }

    #[test]
    fn hybrid_search_similar_pages_and_ai_history_filter_privacy_before_sql_limits() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::database::migrations::run_migrations(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO sources(id,browser,profile,source_type,history_path) VALUES(1,'chrome','Default','sqlite','synthetic')",
            [],
        )
        .unwrap();
        add_privacy_rule(&conn, "private.example", "private").unwrap();
        add_privacy_rule(&conn, "hidden.example", "hidden").unwrap();

        conn.execute(
            "INSERT INTO urls(id,url,title,domain) VALUES(1,'https://public.example/rust-guide','Rust Needle Guide','public.example')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO visits(id,source_id,url_id,source_visit_id,visit_time,event_hash,imported_at) VALUES(1,1,1,1,100,'pub-1',0)",
            [],
        )
        .unwrap();
        save_page_embeddings(&conn, &[(1, "public".into(), vec![1.0, 0.0], "fixture")]).unwrap();

        conn.execute(
            "INSERT INTO urls(id,url,title,domain) VALUES(2,'https://public.example/rust-anchor','Rust Anchor','public.example')",
            [],
        )
        .unwrap();
        save_page_embeddings(&conn, &[(2, "anchor".into(), vec![0.99, 0.01], "fixture")]).unwrap();

        for idx in 10..=135i64 {
            let (url, domain) = if idx % 3 == 0 {
                (
                    format!("http://127.0.0.1/item/{idx}"),
                    "127.0.0.1".to_string(),
                )
            } else if idx % 3 == 1 {
                (
                    format!("https://private.example/item/{idx}"),
                    "private.example".to_string(),
                )
            } else {
                (format!("file:///C:/synthetic/{idx}.html"), String::new())
            };
            conn.execute(
                "INSERT INTO urls(id,url,title,domain) VALUES(?1,?2,'Rust Needle Secret',?3)",
                params![idx, url, domain],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO visits(id,source_id,url_id,source_visit_id,visit_time,event_hash,imported_at) VALUES(?1,1,?1,?1,?2,?3,0)",
                params![idx, idx * 1000, format!("priv-{idx}")],
            )
            .unwrap();
            save_page_embeddings(&conn, &[(idx, "priv".into(), vec![1.0, 0.0], "fixture")])
                .unwrap();
        }

        let hybrid_fts = hybrid_search_history(&conn, "Needle", None, "fixture", 10).unwrap();
        assert_eq!(hybrid_fts.len(), 1);
        assert_eq!(hybrid_fts[0].url, "https://public.example/rust-guide");

        let hybrid =
            hybrid_search_history(&conn, "Needle", Some(&[1.0, 0.0]), "fixture", 10).unwrap();
        assert_eq!(hybrid.len(), 2);
        assert_eq!(hybrid[0].url, "https://public.example/rust-guide");
        assert!(hybrid
            .iter()
            .all(|item| item.url.starts_with("https://public.example/")));

        let similar = find_similar_pages(&conn, 2, "fixture", 10).unwrap();
        assert_eq!(similar.len(), 1);
        assert_eq!(similar[0].url, "https://public.example/rust-guide");

        let local_history = query_history(
            &conn,
            &HistoryFilter {
                limit: Some(10),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(local_history
            .items
            .iter()
            .any(|item| item.domain == "private.example"));

        let ai_history = query_history(
            &conn,
            &HistoryFilter {
                limit: Some(10),
                ai_scope: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(ai_history.items.len(), 1);
        assert_eq!(ai_history.items[0].url, "https://public.example/rust-guide");
    }

    #[test]
    fn on_this_day_rejects_non_ascii_and_out_of_range_dates_without_panic() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::database::migrations::run_migrations(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO sources(id,browser,profile,source_type,history_path) VALUES(1,'chrome','Default','sqlite','synthetic')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO urls(id,url,title,domain) VALUES(1,'https://example.test/','Ex','example.test')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO visits(id,source_id,url_id,source_visit_id,visit_time,event_hash,imported_at) VALUES(1,1,1,1,1700000000000,'h1',0)",
            [],
        )
        .unwrap();

        assert!(get_on_this_day(&conn, Some("2026年11月19日".into()), None, None, None).is_err());
        assert!(get_on_this_day(&conn, Some("9999-01-01".into()), None, None, None).is_err());
        assert!(get_on_this_day(&conn, Some("1800-01-01".into()), None, None, None).is_err());
        assert!(get_on_this_day(&conn, Some("2026-11-19".into()), None, None, None).is_ok());
    }

    #[test]
    fn readonly_topic_and_evolution_queries_respect_privacy_and_query_only() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::database::migrations::run_migrations(&mut conn).unwrap();
        crate::ai::topics::ensure_default_topics(&conn).unwrap();

        let now_ms = chrono::Utc::now().timestamp_millis();
        let day_ms = 86_400_000i64;
        conn.execute_batch(
            "INSERT INTO sources(id,browser,profile,source_type,history_path) VALUES(1,'chrome','Default','sqlite','synthetic');
             INSERT INTO urls(id,url,title,domain) VALUES
               (1,'https://github.com/rust-lang/rust','GitHub Rust','github.com'),
               (2,'https://secret.github.com/private','GitHub Private','secret.github.com');",
        )
        .unwrap();
        add_privacy_rule(&conn, "secret.github.com", "hidden").unwrap();
        conn.execute(
            "INSERT INTO visits(id,source_id,url_id,source_visit_id,visit_time,event_hash,imported_at) VALUES
               (1,1,1,1,?1,'h1',0),
               (2,1,1,2,?2,'h2',0),
               (3,1,1,3,?3,'h3',0),
               (4,1,2,4,?1,'h4',0);",
            rusqlite::params![now_ms - day_ms, now_ms - 2 * day_ms, now_ms - 40 * day_ms],
        )
        .unwrap();

        let tagged = crate::ai::topics::auto_tag_unclassified_urls(&conn, 100).unwrap();
        assert!(tagged > 0);

        conn.pragma_update(None, "query_only", true).unwrap();
        let ro_topics = query_all_topics_readonly(&conn).unwrap();
        let ro_evolution = query_interest_evolution_readonly(&conn, 30).unwrap();
        conn.pragma_update(None, "query_only", false).unwrap();

        let rw_topics = get_all_topics(&conn).unwrap();
        let rw_evolution = get_interest_evolution(&conn, 30).unwrap();

        assert_eq!(ro_topics.len(), rw_topics.len());
        let ro_top = &ro_topics[0];
        let rw_top = &rw_topics[0];
        assert_eq!(ro_top.id, rw_top.id);
        assert_eq!(ro_top.visit_count, 3);
        assert_eq!(ro_top.visit_count, rw_top.visit_count);
        assert_eq!(ro_top.url_count, 1);
        assert_eq!(ro_top.url_count, rw_top.url_count);

        assert_eq!(
            ro_evolution.rising_topics.len(),
            rw_evolution.rising_topics.len()
        );
        assert_eq!(ro_evolution.rising_topics[0].current_visits, 2);
        assert_eq!(ro_evolution.rising_topics[0].previous_visits, 1);
    }
}
