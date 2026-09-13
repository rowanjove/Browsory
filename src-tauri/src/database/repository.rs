use rusqlite::{params, Connection, OptionalExtension};
use tracing::info;

use super::models::{
    AnalyticsSummary, BackupInfo, BrowserStat, DailyTrendStat, DomainDetail, DomainDynamics,
    DomainStat, DomainTopUrl, DormantDomainItem, EmbeddingIndexingStatus, ForgottenGemItem,
    HistoryFilter, HistoryPageResult, HourlyStat, HybridSearchResult, IntegrityReport,
    InterestEvolution, NewDomainItem, OnThisDayItem, OnThisDayResult, PathTreeNode,
    PeriodComparison, PrivacyRule, ResearchSession, SearchQueryStat, SecurityState,
    SimilarPageItem, SmartCollection, Source, TagItem, TopicItem, TopicTrend, VisitDetail,
    VisitListItem, WebMemoryCitation, WebsiteRankingItem, WeeklyHeatmapPoint,
};
use crate::error::{AppError, AppResult};

pub fn list_sources(conn: &Connection) -> AppResult<Vec<Source>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, browser, profile, source_type, history_path, db_fingerprint,
               last_visit_id, last_visit_time, last_sync_at, enabled
        FROM sources
        ORDER BY browser ASC, profile ASC
        "#,
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(Source {
            id: row.get(0)?,
            browser: row.get(1)?,
            profile: row.get(2)?,
            source_type: row.get(3)?,
            history_path: row.get(4)?,
            db_fingerprint: row.get(5)?,
            last_visit_id: row.get(6)?,
            last_visit_time: row.get(7)?,
            last_sync_at: row.get(8)?,
            enabled: row.get::<_, i64>(9)? == 1,
        })
    })?;

    let mut sources = Vec::new();
    for r in rows {
        sources.push(r?);
    }
    Ok(sources)
}

pub fn get_or_create_source(
    conn: &Connection,
    browser: &str,
    profile: &str,
    source_type: &str,
    history_path: &str,
) -> AppResult<Source> {
    let existing: Option<Source> = conn
        .query_row(
            r#"
            SELECT id, browser, profile, source_type, history_path, db_fingerprint,
                   last_visit_id, last_visit_time, last_sync_at, enabled
            FROM sources
            WHERE browser = ?1 AND profile = ?2 AND history_path = ?3
            "#,
            params![browser, profile, history_path],
            |row| {
                Ok(Source {
                    id: row.get(0)?,
                    browser: row.get(1)?,
                    profile: row.get(2)?,
                    source_type: row.get(3)?,
                    history_path: row.get(4)?,
                    db_fingerprint: row.get(5)?,
                    last_visit_id: row.get(6)?,
                    last_visit_time: row.get(7)?,
                    last_sync_at: row.get(8)?,
                    enabled: row.get::<_, i64>(9)? == 1,
                })
            },
        )
        .optional()?;

    if let Some(src) = existing {
        return Ok(src);
    }

    conn.execute(
        r#"
        INSERT INTO sources (browser, profile, source_type, history_path)
        VALUES (?1, ?2, ?3, ?4)
        "#,
        params![browser, profile, source_type, history_path],
    )?;

    let id = conn.last_insert_rowid();
    Ok(Source {
        id,
        browser: browser.to_string(),
        profile: profile.to_string(),
        source_type: source_type.to_string(),
        history_path: history_path.to_string(),
        db_fingerprint: None,
        last_visit_id: 0,
        last_visit_time: 0,
        last_sync_at: None,
        enabled: true,
    })
}

pub fn get_source_by_id(conn: &Connection, source_id: i64) -> AppResult<Option<Source>> {
    conn.query_row(
        r#"
        SELECT id, browser, profile, source_type, history_path, db_fingerprint,
               last_visit_id, last_visit_time, last_sync_at, enabled
        FROM sources
        WHERE id = ?1
        "#,
        params![source_id],
        |row| {
            Ok(Source {
                id: row.get(0)?,
                browser: row.get(1)?,
                profile: row.get(2)?,
                source_type: row.get(3)?,
                history_path: row.get(4)?,
                db_fingerprint: row.get(5)?,
                last_visit_id: row.get(6)?,
                last_visit_time: row.get(7)?,
                last_sync_at: row.get(8)?,
                enabled: row.get::<_, i64>(9)? == 1,
            })
        },
    )
    .optional()
    .map_err(AppError::from)
}

pub fn update_source_sync(
    conn: &Connection,
    source_id: i64,
    last_visit_id: i64,
    last_visit_time: i64,
    last_sync_at: i64,
    db_fingerprint: Option<&str>,
) -> AppResult<()> {
    conn.execute(
        r#"
        UPDATE sources
        SET last_visit_id = MAX(last_visit_id, ?1),
            last_visit_time = MAX(last_visit_time, ?2),
            last_sync_at = ?3,
            db_fingerprint = COALESCE(?5, db_fingerprint)
        WHERE id = ?4
        "#,
        params![
            last_visit_id,
            last_visit_time,
            last_sync_at,
            source_id,
            db_fingerprint
        ],
    )?;
    Ok(())
}

struct AdvancedSearchCriteria {
    fts_tokens: Vec<String>,
    like_query: String,
    site: Option<String>,
    browser: Option<String>,
    profile: Option<String>,
    title: Option<String>,
    url_fragment: Option<String>,
    after_ms: Option<i64>,
    before_ms: Option<i64>,
}

fn parse_advanced_search(raw: &str) -> AdvancedSearchCriteria {
    let mut fts_tokens = Vec::new();
    let mut plain_words = Vec::new();
    let mut site = None;
    let mut browser = None;
    let mut profile = None;
    let mut title = None;
    let mut url_fragment = None;
    let mut after_ms = None;
    let mut before_ms = None;

    for part in raw.split_whitespace() {
        if let Some(val) = part.strip_prefix("site:") {
            if !val.is_empty() {
                site = Some(val.to_lowercase());
            }
        } else if let Some(val) = part.strip_prefix("browser:") {
            if !val.is_empty() {
                browser = Some(val.to_lowercase());
            }
        } else if let Some(val) = part.strip_prefix("profile:") {
            if !val.is_empty() {
                profile = Some(val.to_string());
            }
        } else if let Some(val) = part.strip_prefix("title:") {
            if !val.is_empty() {
                title = Some(val.to_string());
            }
        } else if let Some(val) = part.strip_prefix("url:") {
            if !val.is_empty() {
                url_fragment = Some(val.to_string());
            }
        } else if let Some(val) = part.strip_prefix("after:") {
            if let Ok(d) = chrono::NaiveDate::parse_from_str(val, "%Y-%m-%d") {
                if let Some(dt) = d.and_hms_opt(0, 0, 0) {
                    after_ms = Some(dt.and_utc().timestamp_millis());
                }
            }
        } else if let Some(val) = part.strip_prefix("before:") {
            if let Ok(d) = chrono::NaiveDate::parse_from_str(val, "%Y-%m-%d") {
                if let Some(dt) = d.and_hms_opt(23, 59, 59) {
                    before_ms = Some(dt.and_utc().timestamp_millis());
                }
            }
        } else {
            plain_words.push(part);
            let cleaned: String = part
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == '.')
                .collect();
            if !cleaned.is_empty() {
                fts_tokens.push(format!("\"{}\"*", cleaned.replace('"', "\"\"")));
            }
        }
    }

    AdvancedSearchCriteria {
        fts_tokens,
        like_query: plain_words.join(" "),
        site,
        browser,
        profile,
        title,
        url_fragment,
        after_ms,
        before_ms,
    }
}

pub fn query_history(conn: &Connection, filter: &HistoryFilter) -> AppResult<HistoryPageResult> {
    let limit = filter.limit.unwrap_or(100).min(500) as usize;
    let fetch_limit = limit + 1; // Extra 1 to check has_more

    let mut conditions = Vec::new();
    let mut param_values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    // 1. Advanced Search parser & conditions
    if let Some(ref raw) = filter.search {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            let criteria = parse_advanced_search(trimmed);

            if let Some(site) = criteria.site {
                conditions.push("(u.domain LIKE ? OR u.url LIKE ?)".to_string());
                let pat = format!("%{}%", site);
                param_values.push(Box::new(pat.clone()));
                param_values.push(Box::new(pat));
            }

            if let Some(br) = criteria.browser {
                conditions.push("s.browser LIKE ?".to_string());
                param_values.push(Box::new(format!("%{}%", br)));
            }

            if let Some(prof) = criteria.profile {
                conditions.push("s.profile LIKE ?".to_string());
                param_values.push(Box::new(format!("%{}%", prof)));
            }

            if let Some(t) = criteria.title {
                conditions.push("u.title LIKE ?".to_string());
                param_values.push(Box::new(format!("%{}%", t)));
            }

            if let Some(u_frag) = criteria.url_fragment {
                conditions.push("u.url LIKE ?".to_string());
                param_values.push(Box::new(format!("%{}%", u_frag)));
            }

            if let Some(aft) = criteria.after_ms {
                conditions.push("v.visit_time >= ?".to_string());
                param_values.push(Box::new(aft));
            }

            if let Some(bef) = criteria.before_ms {
                conditions.push("v.visit_time <= ?".to_string());
                param_values.push(Box::new(bef));
            }

            if !criteria.fts_tokens.is_empty() {
                let fts_query = criteria.fts_tokens.join(" ");
                let like_pat = format!("%{}%", criteria.like_query);
                conditions.push("(u.id IN (SELECT rowid FROM urls_fts WHERE urls_fts MATCH ?) OR u.title LIKE ? OR u.url LIKE ?)".to_string());
                param_values.push(Box::new(fts_query));
                param_values.push(Box::new(like_pat.clone()));
                param_values.push(Box::new(like_pat));
            } else if !criteria.like_query.is_empty() {
                let like_pat = format!("%{}%", criteria.like_query);
                conditions.push("(u.title LIKE ? OR u.url LIKE ?)".to_string());
                param_values.push(Box::new(like_pat.clone()));
                param_values.push(Box::new(like_pat));
            }
        }
    }

    // 2. Browsers filter
    if let Some(ref browsers) = filter.browsers {
        if !browsers.is_empty() {
            let placeholders = browsers.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            conditions.push(format!("s.browser IN ({})", placeholders));
            for b in browsers {
                param_values.push(Box::new(b.clone()));
            }
        }
    }

    // 2b. Profiles filter
    if let Some(ref profiles) = filter.profiles {
        if !profiles.is_empty() {
            let placeholders = profiles.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            conditions.push(format!("s.profile IN ({})", placeholders));
            for p in profiles {
                param_values.push(Box::new(p.clone()));
            }
        }
    }

    // 3. Time range filter
    if let Some(st) = filter.start_time {
        conditions.push("v.visit_time >= ?".to_string());
        param_values.push(Box::new(st));
    }
    if let Some(et) = filter.end_time {
        conditions.push("v.visit_time <= ?".to_string());
        param_values.push(Box::new(et));
    }

    // 4. Favorites filter
    if let Some(true) = filter.only_favorites {
        conditions.push("EXISTS (SELECT 1 FROM favorites f WHERE f.url_id = u.id)".to_string());
    }

    // 5. Tag filter
    if let Some(ref tag_name) = filter.tag {
        conditions.push("EXISTS (SELECT 1 FROM url_tags ut JOIN tags t ON ut.tag_id = t.id WHERE ut.url_id = u.id AND t.name = ?)".to_string());
        param_values.push(Box::new(tag_name.clone()));
    }

    // 5b. Exclude Hidden privacy rules from history queries (Fail-closed)
    let active_rules = get_active_privacy_rules(conn)?;
    build_hidden_rule_conditions(
        &active_rules,
        "u.domain",
        "u.url",
        &mut conditions,
        &mut param_values,
    );

    let sort_by = filter.sort_by.as_deref().unwrap_or("time_desc");

    // 6. Pagination
    if filter.offset.is_none() {
        if let (Some(cur_time), Some(cur_id)) = (filter.cursor_time, filter.cursor_id) {
            if sort_by == "time_asc" {
                conditions
                    .push("(v.visit_time > ? OR (v.visit_time = ? AND v.id > ?))".to_string());
            } else {
                conditions
                    .push("(v.visit_time < ? OR (v.visit_time = ? AND v.id < ?))".to_string());
            }
            param_values.push(Box::new(cur_time));
            param_values.push(Box::new(cur_time));
            param_values.push(Box::new(cur_id));
        }
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let order_clause = match sort_by {
        "time_asc" => "ORDER BY v.visit_time ASC, v.id ASC",
        "visit_count" => "ORDER BY (SELECT count(*) FROM visits v_cnt WHERE v_cnt.url_id = u.id) DESC, v.visit_time DESC, v.id DESC",
        "duration" => "ORDER BY v.visit_duration DESC, v.visit_time DESC, v.id DESC",
        _ => "ORDER BY v.visit_time DESC, v.id DESC",
    };

    let (pagination_sql, has_offset) = if filter.offset.is_some() {
        ("LIMIT ? OFFSET ?", true)
    } else {
        ("LIMIT ?", false)
    };

    let sql = format!(
        r#"
        SELECT v.id, v.url_id, v.visit_time, COALESCE(u.title, '') as title, u.url, COALESCE(u.domain, '') as domain,
               s.browser, s.profile, v.visit_duration, v.transition,
               EXISTS(SELECT 1 FROM favorites f WHERE f.url_id = u.id) as is_favorite
        FROM visits v
        JOIN urls u ON v.url_id = u.id
        JOIN sources s ON v.source_id = s.id
        {}
        {}
        {}
        "#,
        where_clause, order_clause, pagination_sql
    );

    param_values.push(Box::new(fetch_limit as i64));
    if has_offset {
        param_values.push(Box::new(filter.offset.unwrap() as i64));
    }

    let mut stmt = conn.prepare(&sql)?;
    let rusqlite_params: Vec<&dyn rusqlite::ToSql> =
        param_values.iter().map(|b| b.as_ref()).collect();

    let rows = stmt.query_map(rusqlite_params.as_slice(), |row| {
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

    let mut items = Vec::new();
    for r in rows {
        items.push(r?);
    }

    let has_more = items.len() > limit;
    if has_more {
        items.truncate(limit);
    }

    let (next_cursor_time, next_cursor_id) = if let Some(last_item) = items.last() {
        if has_more {
            (Some(last_item.visit_time), Some(last_item.id))
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    Ok(HistoryPageResult {
        items,
        next_cursor_time,
        next_cursor_id,
        has_more,
    })
}

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
    if visit_ids.is_empty() {
        return Ok(0);
    }

    let mut total_deleted = 0;
    // Chunk by 500 to avoid SQLite SQLITE_MAX_VARIABLE_NUMBER limits
    for chunk in visit_ids.chunks(500) {
        let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!("DELETE FROM visits WHERE id IN ({})", placeholders);

        let params: Vec<&dyn rusqlite::ToSql> =
            chunk.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
        let deleted = conn.execute(&sql, params.as_slice())?;
        total_deleted += deleted;
    }

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

pub fn get_analytics_summary(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    days: Option<u32>,
) -> AppResult<AnalyticsSummary> {
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
    let hidden_clause = if hidden_conds.is_empty() {
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

    // 1. Total visits
    let total_visits: u64 = conn
        .query_row(
            &format!(
                "SELECT count(*) FROM visits v JOIN urls u ON v.url_id = u.id WHERE v.visit_time >= ? AND v.visit_time <= ? {}",
                hidden_clause
            ),
            time_params.as_slice(),
            |row| row.get(0),
        )
        .unwrap_or(0);

    // 1b. Unique URLs
    let unique_urls: u64 = conn
        .query_row(
            &format!(
                "SELECT count(DISTINCT u.id) FROM visits v JOIN urls u ON v.url_id = u.id WHERE v.visit_time >= ? AND v.visit_time <= ? {}",
                hidden_clause
            ),
            time_params.as_slice(),
            |row| row.get(0),
        )
        .unwrap_or(0);

    // 1c. Unique Domains
    let unique_domains: u64 = conn
        .query_row(
            &format!(
                r#"
                SELECT count(DISTINCT u.domain)
                FROM visits v
                JOIN urls u ON v.url_id = u.id
                WHERE v.visit_time >= ? AND v.visit_time <= ? AND u.domain IS NOT NULL AND u.domain != '' {}
                "#,
                hidden_clause
            ),
            time_params.as_slice(),
            |row| row.get(0),
        )
        .unwrap_or(0);

    // 1d. Active Days
    let active_days: u64 = conn
        .query_row(
            &format!(
                r#"
                SELECT count(DISTINCT strftime('%Y-%m-%d', datetime(v.visit_time / 1000, 'unixepoch', 'localtime')))
                FROM visits v
                JOIN urls u ON v.url_id = u.id
                WHERE v.visit_time >= ? AND v.visit_time <= ? {}
                "#,
                hidden_clause
            ),
            time_params.as_slice(),
            |row| row.get(0),
        )
        .unwrap_or(0);

    // 1e. Revisit Rate
    let revisit_rate = if total_visits > 0 {
        ((total_visits as f64 - unique_urls as f64) / total_visits as f64 * 1000.0).round() / 10.0
    } else {
        0.0
    }
    .max(0.0);

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

    // 4. Hourly distribution (0..23)
    let mut hourly_map = std::collections::HashMap::new();
    let hourly_sql = format!(
        r#"
        SELECT CAST(strftime('%H', datetime(v.visit_time / 1000, 'unixepoch', 'localtime')) AS INTEGER) as hr, count(v.id)
        FROM visits v
        JOIN urls u ON v.url_id = u.id
        WHERE v.visit_time >= ? AND v.visit_time <= ? {}
        GROUP BY hr
        "#,
        hidden_clause
    );
    let mut hourly_stmt = conn.prepare(&hourly_sql)?;
    let hourly_rows = hourly_stmt.query_map(time_params.as_slice(), |row| {
        Ok((row.get::<_, u32>(0)?, row.get::<_, u64>(1)?))
    })?;
    for (hr, cnt) in hourly_rows.flatten() {
        hourly_map.insert(hr, cnt);
    }
    let mut hourly_distribution = Vec::with_capacity(24);
    for h in 0..24 {
        hourly_distribution.push(HourlyStat {
            hour: h,
            count: *hourly_map.get(&h).unwrap_or(&0),
        });
    }

    // 5. Daily trend
    let daily_sql = format!(
        r#"
        SELECT strftime('%Y-%m-%d', datetime(v.visit_time / 1000, 'unixepoch', 'localtime')) as dt, count(v.id)
        FROM visits v
        JOIN urls u ON v.url_id = u.id
        WHERE v.visit_time >= ? AND v.visit_time <= ? {}
        GROUP BY dt
        ORDER BY dt ASC
        "#,
        hidden_clause
    );
    let mut daily_stmt = conn.prepare(&daily_sql)?;
    let daily_rows = daily_stmt.query_map(time_params.as_slice(), |row| {
        Ok(DailyTrendStat {
            date: row.get(0)?,
            count: row.get(1)?,
        })
    })?;
    let mut daily_trend = Vec::new();
    for r in daily_rows {
        daily_trend.push(r?);
    }

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

    // 7. 7x24 Weekly-Hourly Heatmap (168 points)
    let mut weekly_map = std::collections::HashMap::new();
    let weekly_sql = format!(
        r#"
        SELECT 
            CAST(strftime('%w', datetime(v.visit_time / 1000, 'unixepoch', 'localtime')) AS INTEGER) as dow,
            CAST(strftime('%H', datetime(v.visit_time / 1000, 'unixepoch', 'localtime')) AS INTEGER) as hr,
            count(v.id) as cnt
        FROM visits v
        JOIN urls u ON v.url_id = u.id
        WHERE v.visit_time >= ? AND v.visit_time <= ? {}
        GROUP BY dow, hr
        "#,
        hidden_clause
    );
    let mut weekly_stmt = conn.prepare(&weekly_sql)?;
    let weekly_rows = weekly_stmt.query_map(time_params.as_slice(), |row| {
        Ok((
            row.get::<_, u32>(0)?,
            row.get::<_, u32>(1)?,
            row.get::<_, u64>(2)?,
        ))
    })?;
    for (dow, hr, cnt) in weekly_rows.flatten() {
        weekly_map.insert((dow, hr), cnt);
    }
    let mut weekly_heatmap = Vec::with_capacity(168);
    for dow in 0..7 {
        for hr in 0..24 {
            weekly_heatmap.push(WeeklyHeatmapPoint {
                day_of_week: dow,
                hour: hr,
                count: *weekly_map.get(&(dow, hr)).unwrap_or(&0),
            });
        }
    }

    // 8. Forgotten Gems (高频访问 >= 3 次，但 30 天未再次打开)
    let thirty_days_ago = now_ms - 30 * 86_400_000;
    let gems_sql = format!(
        r#"
        SELECT u.id, u.title, u.url, u.domain, count(v.id) as cnt, max(v.visit_time) as last_time
        FROM urls u
        JOIN visits v ON v.url_id = u.id
        WHERE 1=1 {}
        GROUP BY u.id
        HAVING cnt >= 3 AND last_time < ?
        ORDER BY cnt DESC, last_time DESC
        LIMIT 15
        "#,
        hidden_clause
    );
    let mut gems_stmt = conn.prepare(&gems_sql)?;
    let mut gems_params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(1 + hidden_params.len());
    for b in &hidden_params {
        gems_params.push(b.as_ref());
    }
    gems_params.push(&thirty_days_ago);
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
        let prev_cnt: u64 = conn
            .query_row(
                &format!(
                    "SELECT count(*) FROM visits v JOIN urls u ON v.url_id = u.id WHERE v.visit_time >= ? AND v.visit_time < ? {}",
                    hidden_clause
                ),
                prev_time_params.as_slice(),
                |r| r.get(0),
            )
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

    let mut stmt = conn.prepare(
        r#"
        SELECT v.id, v.source_id, v.visit_time, u.title, u.url, u.domain, s.browser, s.profile
        FROM visits v
        JOIN urls u ON v.url_id = u.id
        JOIN sources s ON v.source_id = s.id
        WHERE v.visit_time >= ?1
        ORDER BY v.source_id ASC, v.visit_time ASC
        LIMIT 10000
        "#,
    )?;

    struct VisitRaw {
        source_id: i64,
        visit_time: i64,
        title: String,
        url: String,
        domain: String,
        browser: String,
        profile: String,
    }

    let rows = stmt.query_map(params![min_visit_time], |row| {
        Ok(VisitRaw {
            source_id: row.get(1)?,
            visit_time: row.get(2)?,
            title: row.get(3)?,
            url: row.get(4)?,
            domain: row.get(5)?,
            browser: row.get(6)?,
            profile: row.get(7)?,
        })
    })?;

    let active_rules = get_active_privacy_rules(conn)?;
    let mut all_visits = Vec::new();
    for r in rows {
        let v = r?;
        let excluded = active_rules.iter().any(|rule| {
            let hidden = rule.rule_type == "hidden";
            // Unknown legacy rule types are treated as AI-blocking so a
            // malformed row can never widen cloud-data exposure.
            let private_for_ai = exclude_private && rule.rule_type != "hidden";
            (hidden || private_for_ai) && matches_privacy_rule(&rule.pattern, &v.domain, &v.url)
        });
        if excluded {
            continue;
        }
        all_visits.push(v);
    }

    struct ActiveSession {
        source_id: i64,
        start_time: i64,
        end_time: i64,
        visit_count: u32,
        domain_counts: std::collections::HashMap<String, u32>,
        first_title: String,
        browser: String,
        profile: String,
    }

    let mut sessions: Vec<ResearchSession> = Vec::new();
    let mut cur_session: Option<ActiveSession> = None;

    let flush_session = |s: ActiveSession, out: &mut Vec<ResearchSession>| {
        let mut dom_vec: Vec<_> = s.domain_counts.into_iter().collect();
        dom_vec.sort_by(|a, b| b.1.cmp(&a.1));
        let dominant_domain = dom_vec.first().map(|(d, _)| d.clone()).unwrap_or_default();
        let duration_secs = ((s.end_time - s.start_time) / 1000).max(0);
        let sample_title = if s.first_title.trim().is_empty() {
            dominant_domain.clone()
        } else {
            s.first_title
        };
        let session_id = format!("ses_{}_{}", s.source_id, s.start_time);

        out.push(ResearchSession {
            session_id,
            start_time: s.start_time,
            end_time: s.end_time,
            duration_secs,
            visit_count: s.visit_count,
            dominant_domain,
            sample_title,
            browser: s.browser,
            profile: s.profile,
        });
    };

    const SESSION_BREAK_MS: i64 = 30 * 60 * 1000; // 30 minutes

    for v in all_visits {
        match cur_session.take() {
            None => {
                let mut dmap = std::collections::HashMap::new();
                if !v.domain.is_empty() {
                    dmap.insert(v.domain.clone(), 1);
                }
                cur_session = Some(ActiveSession {
                    source_id: v.source_id,
                    start_time: v.visit_time,
                    end_time: v.visit_time,
                    visit_count: 1,
                    domain_counts: dmap,
                    first_title: v.title,
                    browser: v.browser,
                    profile: v.profile,
                });
            }
            Some(mut s) => {
                if v.source_id == s.source_id && (v.visit_time - s.end_time) <= SESSION_BREAK_MS {
                    if !v.domain.is_empty() {
                        *s.domain_counts.entry(v.domain.clone()).or_insert(0) += 1;
                    }
                    s.end_time = v.visit_time;
                    s.visit_count += 1;
                    cur_session = Some(s);
                } else {
                    flush_session(s, &mut sessions);
                    let mut new_dmap = std::collections::HashMap::new();
                    if !v.domain.is_empty() {
                        new_dmap.insert(v.domain.clone(), 1);
                    }
                    cur_session = Some(ActiveSession {
                        source_id: v.source_id,
                        start_time: v.visit_time,
                        end_time: v.visit_time,
                        visit_count: 1,
                        domain_counts: new_dmap,
                        first_title: v.title,
                        browser: v.browser,
                        profile: v.profile,
                    });
                }
            }
        }
    }

    if let Some(last_s) = cur_session {
        flush_session(last_s, &mut sessions);
    }

    sessions.sort_by(|a, b| b.start_time.cmp(&a.start_time));
    if limit > 0 && sessions.len() > limit as usize {
        sessions.truncate(limit as usize);
    }

    Ok(sessions)
}

// ==========================================
// Security & App Lock Repository Methods
// ==========================================

pub fn get_security_state(conn: &Connection) -> AppResult<SecurityState> {
    let now = chrono::Utc::now().timestamp_millis();
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

            Ok(SecurityState {
                pin_enabled: pin_enabled == 1,
                auto_lock_minutes,
                lock_on_minimize: lock_on_minimize == 1,
                lock_on_sleep: lock_on_sleep == 1,
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

pub fn matches_privacy_rule(pattern: &str, domain: &str, url: &str) -> bool {
    let d_lower = domain.to_lowercase();
    let u_lower = url.to_lowercase();
    let p_lower = pattern.to_lowercase();
    if let Some(suffix) = p_lower.strip_prefix("*.") {
        d_lower == suffix || d_lower.ends_with(&format!(".{}", suffix))
    } else {
        d_lower.contains(&p_lower) || u_lower.contains(&p_lower)
    }
}

fn escape_like_literal(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

pub fn build_hidden_rule_conditions(
    rules: &[PrivacyRule],
    domain_col: &str,
    url_col: &str,
    conditions: &mut Vec<String>,
    param_values: &mut Vec<Box<dyn rusqlite::ToSql>>,
) {
    for r in rules.iter().filter(|r| r.rule_type == "hidden") {
        let pat = r.pattern.to_lowercase();
        if let Some(suffix) = pat.strip_prefix("*.") {
            conditions.push(format!(
                r#"NOT (COALESCE({}, '') LIKE ? ESCAPE '\' OR COALESCE({}, '') = ?)"#,
                domain_col, domain_col
            ));
            param_values.push(Box::new(format!("%.{}", escape_like_literal(suffix))));
            param_values.push(Box::new(suffix.to_string()));
        } else {
            conditions.push(format!(
                r#"NOT (COALESCE({}, '') LIKE ? ESCAPE '\' OR COALESCE({}, '') LIKE ? ESCAPE '\')"#,
                domain_col, url_col
            ));
            let escaped = escape_like_literal(&pat);
            param_values.push(Box::new(format!("%{}%", escaped)));
            param_values.push(Box::new(format!("%{}%", escaped)));
        }
    }
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
    let active_rules = get_active_privacy_rules(conn)?;
    let is_blocked = |domain: &str, url: &str| -> bool {
        active_rules
            .iter()
            .any(|rule| matches_privacy_rule(&rule.pattern, domain, url))
    };

    let trimmed = query.trim();
    let rows = if trimmed.is_empty() {
        let mut stmt = conn.prepare(
            r#"
            SELECT v.id, u.title, u.url, u.domain, v.visit_time
            FROM visits v
            JOIN urls u ON v.url_id = u.id
            ORDER BY v.visit_time DESC
            LIMIT 100
            "#,
        )?;
        let iter = stmt.query_map([], |row| {
            Ok(WebMemoryCitation {
                visit_id: row.get(0)?,
                title: row.get(1)?,
                url: row.get(2)?,
                domain: row.get(3)?,
                visit_time: row.get(4)?,
            })
        })?;
        let mut list = Vec::new();
        for r in iter {
            list.push(r?);
        }
        list
    } else {
        let mut list = Vec::new();
        let cleaned: String = trimmed
            .chars()
            .filter(|c| {
                c.is_alphanumeric() || c.is_whitespace() || *c == '_' || *c == '-' || *c == '.'
            })
            .collect();
        let fts_tokens: Vec<String> = cleaned
            .split_whitespace()
            .filter(|w| !w.is_empty())
            .map(|w| format!("\"{}\"*", w.replace('"', "\"\"")))
            .collect();

        if !fts_tokens.is_empty() {
            let fts_query = fts_tokens.join(" ");
            if let Ok(mut fts_stmt) = conn.prepare(
                r#"
                SELECT v.id, u.title, u.url, u.domain, v.visit_time
                FROM urls_fts fts
                JOIN urls u ON fts.rowid = u.id
                JOIN visits v ON v.url_id = u.id
                WHERE urls_fts MATCH ?1
                ORDER BY v.visit_time DESC
                LIMIT 80
                "#,
            ) {
                if let Ok(iter) = fts_stmt.query_map(params![fts_query], |row| {
                    Ok(WebMemoryCitation {
                        visit_id: row.get(0)?,
                        title: row.get(1)?,
                        url: row.get(2)?,
                        domain: row.get(3)?,
                        visit_time: row.get(4)?,
                    })
                }) {
                    for r in iter.flatten() {
                        list.push(r);
                    }
                }
            }
        }

        if list.len() < limit as usize {
            let like_pattern = format!("%{}%", trimmed);
            let mut like_stmt = conn.prepare(
                r#"
                SELECT v.id, u.title, u.url, u.domain, v.visit_time
                FROM visits v
                JOIN urls u ON v.url_id = u.id
                WHERE u.title LIKE ?1 OR u.url LIKE ?1 OR u.domain LIKE ?1
                ORDER BY v.visit_time DESC
                LIMIT 60
                "#,
            )?;
            let iter = like_stmt.query_map(params![like_pattern], |row| {
                Ok(WebMemoryCitation {
                    visit_id: row.get(0)?,
                    title: row.get(1)?,
                    url: row.get(2)?,
                    domain: row.get(3)?,
                    visit_time: row.get(4)?,
                })
            })?;
            for r in iter.flatten() {
                if !list.iter().any(|item| item.visit_id == r.visit_id) {
                    list.push(r);
                }
            }
        }
        list
    };

    let mut safe_citations = Vec::new();
    let mut seen_urls = std::collections::HashSet::new();

    for item in rows {
        if is_blocked(&item.domain, &item.url) {
            continue;
        }
        if seen_urls.insert(item.url.clone()) {
            safe_citations.push(item);
        }
        if safe_citations.len() >= limit as usize {
            break;
        }
    }

    Ok(safe_citations)
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

    let sql = format!(
        r#"
        SELECT 
            u.domain,
            count(v.id) as visits,
            count(DISTINCT u.id) as unique_urls,
            count(DISTINCT strftime('%Y-%m-%d', datetime(v.visit_time / 1000, 'unixepoch', 'localtime'))) as active_days,
            min(v.visit_time) as first_visit,
            max(v.visit_time) as last_visit
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
    let mut query_params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(3 + hidden_params.len());
    query_params.push(&min_time);
    query_params.push(&max_time);
    for p in &hidden_params {
        query_params.push(p.as_ref());
    }
    query_params.push(&top_limit);

    let rows = stmt.query_map(query_params.as_slice(), |row| {
        let domain: String = row.get(0)?;
        let visits: u64 = row.get(1)?;
        let unique_urls: u64 = row.get(2)?;
        let active_days: u64 = row.get(3)?;
        let first_visit_time: i64 = row.get(4)?;
        let last_visit_time: i64 = row.get(5)?;
        let revisit_rate = if visits > 0 {
            ((visits as f64 - unique_urls as f64) / visits as f64 * 1000.0).round() / 10.0
        } else {
            0.0
        };

        Ok(WebsiteRankingItem {
            domain,
            visits,
            unique_urls,
            active_days,
            revisit_rate: revisit_rate.max(0.0),
            first_visit_time,
            last_visit_time,
        })
    })?;

    let mut list = Vec::new();
    for r in rows {
        list.push(r?);
    }
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
        children.sort_by(|a, b| b.visits.cmp(&a.visits));
        PathTreeNode {
            name: node.name,
            full_path: node.full_path,
            visits: node.visits,
            unique_urls: node.urls.len() as u64,
            children,
        }
    }

    let mut result: Vec<PathTreeNode> = root_children.into_values().map(convert).collect();
    result.sort_by(|a, b| b.visits.cmp(&a.visits));
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

    // New Domains: domain whose min(v.visit_time) overall is within [min_time, max_time]
    let new_sql = format!(
        r#"
        SELECT u.domain, min(v.visit_time) as first_visit, COALESCE(u.title, ''), u.url, count(v.id) as cnt
        FROM visits v
        JOIN urls u ON v.url_id = u.id
        WHERE u.domain IS NOT NULL AND u.domain != '' {}
        GROUP BY u.domain
        HAVING first_visit >= ? AND first_visit <= ?
        ORDER BY cnt DESC, first_visit DESC
        LIMIT 40
        "#,
        hidden_clause
    );
    let mut new_stmt = conn.prepare(&new_sql)?;
    let mut new_params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(hidden_params.len() + 2);
    for p in &hidden_params {
        new_params.push(p.as_ref());
    }
    new_params.push(&min_time);
    new_params.push(&max_time);
    let new_rows = new_stmt.query_map(new_params.as_slice(), |row| {
        Ok(NewDomainItem {
            domain: row.get(0)?,
            first_visit_time: row.get(1)?,
            sample_title: row.get(2)?,
            sample_url: row.get(3)?,
            visits_in_period: row.get(4)?,
        })
    })?;
    let mut new_domains = Vec::new();
    for r in new_rows {
        let item = r?;
        new_domains.push(item);
        if new_domains.len() >= 20 {
            break;
        }
    }

    // Dormant Domains: domain with >= 5 historical visits, but last_visit < min_time
    let dormant_sql = format!(
        r#"
        SELECT u.domain, count(v.id) as total_cnt, max(v.visit_time) as last_time
        FROM visits v
        JOIN urls u ON v.url_id = u.id
        WHERE u.domain IS NOT NULL AND u.domain != '' {}
        GROUP BY u.domain
        HAVING total_cnt >= 5 AND last_time < ?
        ORDER BY total_cnt DESC, last_time DESC
        LIMIT 40
        "#,
        hidden_clause
    );
    let mut dormant_stmt = conn.prepare(&dormant_sql)?;
    let mut dormant_params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(hidden_params.len() + 1);
    for p in &hidden_params {
        dormant_params.push(p.as_ref());
    }
    dormant_params.push(&min_time);
    let dormant_rows = dormant_stmt.query_map(dormant_params.as_slice(), |row| {
        let domain: String = row.get(0)?;
        let total_historical_visits: u64 = row.get(1)?;
        let last_visit_time: i64 = row.get(2)?;
        let days_dormant = if now_ms > last_visit_time {
            ((now_ms - last_visit_time) / 86_400_000) as u64
        } else {
            0
        };
        Ok(DormantDomainItem {
            domain,
            total_historical_visits,
            last_visit_time,
            days_dormant,
        })
    })?;
    let mut dormant_domains = Vec::new();
    for r in dormant_rows {
        let item = r?;
        dormant_domains.push(item);
        if dormant_domains.len() >= 20 {
            break;
        }
    }

    Ok(DomainDynamics {
        new_domains,
        dormant_domains,
    })
}

pub fn get_on_this_day(
    conn: &Connection,
    target_date: Option<String>,
    filter_year: Option<i32>,
    page: Option<u32>,
    page_size: Option<u32>,
) -> AppResult<OnThisDayResult> {
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

    let date_str =
        target_date.unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());

    let (target_year, target_month_day) = if date_str.len() >= 10 {
        let yr: i32 = date_str[0..4].parse().unwrap_or(2026);
        let md = &date_str[5..10];
        (yr, md.to_string())
    } else {
        (2026, "01-01".to_string())
    };

    // 1. 获取所有存在记录的历史年份列表 (倒序排序，覆盖历年)
    let years_sql = format!(
        r#"
        SELECT DISTINCT CAST(strftime('%Y', datetime(v.visit_time / 1000, 'unixepoch', 'localtime')) AS INTEGER) as v_year
        FROM visits v
        JOIN urls u ON v.url_id = u.id
        WHERE strftime('%m-%d', datetime(v.visit_time / 1000, 'unixepoch', 'localtime')) = ?
          AND CAST(strftime('%Y', datetime(v.visit_time / 1000, 'unixepoch', 'localtime')) AS INTEGER) < ? {}
        ORDER BY v_year DESC
        "#,
        hidden_clause
    );
    let mut years_stmt = conn.prepare(&years_sql)?;
    let mut years_params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(2 + hidden_params.len());
    years_params.push(&target_month_day);
    years_params.push(&target_year);
    for b in &hidden_params {
        years_params.push(b.as_ref());
    }
    let years_rows = years_stmt.query_map(years_params.as_slice(), |row| row.get::<_, i32>(0))?;
    let mut available_years = Vec::new();
    for y in years_rows {
        available_years.push(y?);
    }

    let p = page.unwrap_or(1).max(1);
    let ps = page_size.unwrap_or(30).clamp(1, 100);
    let offset = (p - 1) * ps;

    // 2. 统计总数 (全历年或指定年份)
    let total_count: u32 = if let Some(yr) = filter_year {
        let count_sql = format!(
            r#"
            SELECT COUNT(*)
            FROM visits v
            JOIN urls u ON v.url_id = u.id
            WHERE strftime('%m-%d', datetime(v.visit_time / 1000, 'unixepoch', 'localtime')) = ?
              AND CAST(strftime('%Y', datetime(v.visit_time / 1000, 'unixepoch', 'localtime')) AS INTEGER) = ? {}
            "#,
            hidden_clause
        );
        let mut count_stmt = conn.prepare(&count_sql)?;
        let mut count_params: Vec<&dyn rusqlite::ToSql> =
            Vec::with_capacity(2 + hidden_params.len());
        count_params.push(&target_month_day);
        count_params.push(&yr);
        for b in &hidden_params {
            count_params.push(b.as_ref());
        }
        count_stmt.query_row(count_params.as_slice(), |row| row.get(0))?
    } else {
        let count_sql = format!(
            r#"
            SELECT COUNT(*)
            FROM visits v
            JOIN urls u ON v.url_id = u.id
            WHERE strftime('%m-%d', datetime(v.visit_time / 1000, 'unixepoch', 'localtime')) = ?
              AND CAST(strftime('%Y', datetime(v.visit_time / 1000, 'unixepoch', 'localtime')) AS INTEGER) < ? {}
            "#,
            hidden_clause
        );
        let mut count_stmt = conn.prepare(&count_sql)?;
        let mut count_params: Vec<&dyn rusqlite::ToSql> =
            Vec::with_capacity(2 + hidden_params.len());
        count_params.push(&target_month_day);
        count_params.push(&target_year);
        for b in &hidden_params {
            count_params.push(b.as_ref());
        }
        count_stmt.query_row(count_params.as_slice(), |row| row.get(0))?
    };

    let total_pages = if total_count == 0 {
        1
    } else {
        total_count.div_ceil(ps)
    };

    // 3. 分页查询
    let mut items = Vec::new();
    if let Some(yr) = filter_year {
        let page_sql = format!(
            r#"
            SELECT v.id, u.url, COALESCE(u.title, '') as title, COALESCE(u.domain, '') as domain,
                   v.visit_time,
                   CAST(strftime('%Y', datetime(v.visit_time / 1000, 'unixepoch', 'localtime')) AS INTEGER) as v_year
            FROM visits v
            JOIN urls u ON v.url_id = u.id
            WHERE strftime('%m-%d', datetime(v.visit_time / 1000, 'unixepoch', 'localtime')) = ?
              AND CAST(strftime('%Y', datetime(v.visit_time / 1000, 'unixepoch', 'localtime')) AS INTEGER) = ? {}
            ORDER BY v.visit_time DESC
            LIMIT ? OFFSET ?
            "#,
            hidden_clause
        );
        let mut stmt = conn.prepare(&page_sql)?;
        let mut page_params: Vec<&dyn rusqlite::ToSql> =
            Vec::with_capacity(4 + hidden_params.len());
        page_params.push(&target_month_day);
        page_params.push(&yr);
        for b in &hidden_params {
            page_params.push(b.as_ref());
        }
        page_params.push(&ps);
        page_params.push(&offset);

        let rows = stmt.query_map(page_params.as_slice(), |row| {
            let id: i64 = row.get(0)?;
            let url: String = row.get(1)?;
            let title: String = row.get(2)?;
            let domain: String = row.get(3)?;
            let visit_time: i64 = row.get(4)?;
            let v_year: i32 = row.get(5)?;
            let years_ago = (target_year - v_year).max(1) as u32;

            Ok(OnThisDayItem {
                id,
                url,
                title,
                domain,
                visit_time,
                years_ago,
            })
        })?;
        for r in rows {
            items.push(r?);
        }
    } else {
        let page_sql = format!(
            r#"
            SELECT v.id, u.url, COALESCE(u.title, '') as title, COALESCE(u.domain, '') as domain,
                   v.visit_time,
                   CAST(strftime('%Y', datetime(v.visit_time / 1000, 'unixepoch', 'localtime')) AS INTEGER) as v_year
            FROM visits v
            JOIN urls u ON v.url_id = u.id
            WHERE strftime('%m-%d', datetime(v.visit_time / 1000, 'unixepoch', 'localtime')) = ?
              AND CAST(strftime('%Y', datetime(v.visit_time / 1000, 'unixepoch', 'localtime')) AS INTEGER) < ? {}
            ORDER BY v.visit_time DESC
            LIMIT ? OFFSET ?
            "#,
            hidden_clause
        );
        let mut stmt = conn.prepare(&page_sql)?;
        let mut page_params: Vec<&dyn rusqlite::ToSql> =
            Vec::with_capacity(4 + hidden_params.len());
        page_params.push(&target_month_day);
        page_params.push(&target_year);
        for b in &hidden_params {
            page_params.push(b.as_ref());
        }
        page_params.push(&ps);
        page_params.push(&offset);

        let rows = stmt.query_map(page_params.as_slice(), |row| {
            let id: i64 = row.get(0)?;
            let url: String = row.get(1)?;
            let title: String = row.get(2)?;
            let domain: String = row.get(3)?;
            let visit_time: i64 = row.get(4)?;
            let v_year: i32 = row.get(5)?;
            let years_ago = (target_year - v_year).max(1) as u32;

            Ok(OnThisDayItem {
                id,
                url,
                title,
                domain,
                visit_time,
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
    for r in rules {
        let pat = r.pattern.to_lowercase();
        if let Some(suffix) = pat.strip_prefix("*.") {
            exclude_conditions.push(
                r#"NOT (COALESCE(u.domain, '') LIKE ? ESCAPE '\' OR COALESCE(u.domain, '') = ?)"#
                    .to_string(),
            );
            param_values.push(Box::new(format!("%.{}", escape_like_literal(suffix))));
            param_values.push(Box::new(suffix.to_string()));
        } else {
            exclude_conditions.push(
                r#"NOT (COALESCE(u.domain, '') LIKE ? ESCAPE '\' OR COALESCE(u.url, '') LIKE ? ESCAPE '\')"#
                    .to_string(),
            );
            let escaped = escape_like_literal(&pat);
            param_values.push(Box::new(format!("%{}%", escaped)));
            param_values.push(Box::new(format!("%{}%", escaped)));
        }
    }

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
    for r in rules {
        let pat = r.pattern.to_lowercase();
        if let Some(suffix) = pat.strip_prefix("*.") {
            exclude_conditions.push(
                r#"NOT (COALESCE(domain, '') LIKE ? ESCAPE '\' OR COALESCE(domain, '') = ?)"#
                    .to_string(),
            );
            param_values.push(Box::new(format!("%.{}", escape_like_literal(suffix))));
            param_values.push(Box::new(suffix.to_string()));
        } else {
            exclude_conditions.push(
                r#"NOT (COALESCE(domain, '') LIKE ? ESCAPE '\' OR COALESCE(url, '') LIKE ? ESCAPE '\')"#
                    .to_string(),
            );
            let escaped = escape_like_literal(&pat);
            param_values.push(Box::new(format!("%{}%", escaped)));
            param_values.push(Box::new(format!("%{}%", escaped)));
        }
    }

    let where_clause = if exclude_conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", exclude_conditions.join(" AND "))
    };

    let total_sql = format!("SELECT count(*) FROM urls {}", where_clause);
    let mut total_stmt = conn.prepare(&total_sql)?;
    let rusqlite_params: Vec<&dyn rusqlite::ToSql> =
        param_values.iter().map(|b| b.as_ref()).collect();
    let total_urls: u64 = total_stmt.query_row(rusqlite_params.as_slice(), |r| r.get(0))?;

    let indexed_urls: u64 = conn.query_row(
        "SELECT count(*) FROM page_embeddings WHERE model = ?1",
        params![model],
        |r| r.get(0),
    )?;
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

    let mut stmt = conn.prepare(
        r#"
        SELECT pe.url_id, u.url, COALESCE(u.title, ''), COALESCE(u.domain, ''),
               (SELECT count(*) FROM visits v WHERE v.url_id = u.id) as vc,
               COALESCE((SELECT max(v.visit_time) FROM visits v WHERE v.url_id = u.id), 0) as lvt,
               pe.embedding
        FROM page_embeddings pe
        JOIN urls u ON pe.url_id = u.id
        WHERE pe.model = ?1 AND pe.url_id != ?2
        "#,
    )?;

    let rows = stmt.query_map(params![model, url_id], |r| {
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

    let active_rules = get_active_privacy_rules(conn)?;
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

    // 1. FTS search
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
            if let Ok(mut fts_stmt) = conn.prepare(
                r#"
                SELECT u.id, u.url, COALESCE(u.title, ''), COALESCE(u.domain, ''),
                       (SELECT count(*) FROM visits v WHERE v.url_id = u.id) as vc,
                       COALESCE((SELECT max(v.visit_time) FROM visits v WHERE v.url_id = u.id), 0) as lvt
                FROM urls_fts fts
                JOIN urls u ON fts.rowid = u.id
                WHERE urls_fts MATCH ?1
                ORDER BY rank
                LIMIT 100
                "#,
            ) {
                if let Ok(rows) = fts_stmt.query_map(params![fts_query], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, u64>(4)?,
                        r.get::<_, i64>(5)?,
                    ))
                }) {
                    let mut valid_rank = 0;
                    for (id, url, title, domain, vc, lvt) in rows.flatten() {
                        if active_rules.iter().any(|rule| matches_privacy_rule(&rule.pattern, &domain, &url)) {
                            continue;
                        }
                        fts_ranks.insert(id, valid_rank);
                        valid_rank += 1;
                        url_details.insert(id, (url, title, domain, vc, lvt));
                    }
                }
            }
        }
    }

    // 2. Vector search if query_vector is provided
    if let Some(q_vec) = query_vector {
        let mut vec_stmt = conn.prepare(
            r#"
            SELECT pe.url_id, u.url, COALESCE(u.title, ''), COALESCE(u.domain, ''),
                   (SELECT count(*) FROM visits v WHERE v.url_id = u.id) as vc,
                   COALESCE((SELECT max(v.visit_time) FROM visits v WHERE v.url_id = u.id), 0) as lvt,
                   pe.embedding
            FROM page_embeddings pe
            JOIN urls u ON pe.url_id = u.id
            WHERE pe.model = ?1
            "#,
        )?;

        let rows = vec_stmt.query_map(params![model], |r| {
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
        for (id, url, title, domain, vc, lvt, bytes) in rows.flatten() {
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

    // 3. Compute RRF scores
    let all_ids: Vec<i64> = url_details.keys().cloned().collect();
    let mut results = Vec::new();

    for id in all_ids {
        let fts_r = fts_ranks.get(&id).cloned();
        let vec_r = vec_ranks.get(&id).cloned();
        let sim = similarities.get(&id).cloned().unwrap_or(0.0);
        let rrf = crate::ai::embedding::rrf_score(fts_r, vec_r, 60.0);

        if let Some((url, title, domain, vc, lvt)) = url_details.remove(&id) {
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
                rrf_score: (rrf * 10000.0).round() / 10000.0,
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

    let sql = format!(
        r#"
        SELECT t.id, t.name, t.category, t.icon, t.color,
               COUNT(v.id) as visit_count,
               COUNT(DISTINCT CASE WHEN u.id IS NOT NULL THEN ut.url_id END) as url_count
        FROM topics t
        LEFT JOIN url_topics ut ON t.id = ut.topic_id
        LEFT JOIN urls u ON ut.url_id = u.id {}
        LEFT JOIN visits v ON u.id = v.url_id
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
    crate::ai::topics::ensure_default_topics(conn)?;
    crate::ai::topics::auto_tag_unclassified_urls(conn, 300)?;

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

    let now = chrono::Utc::now().timestamp_millis();
    let period_ms = (days as i64) * 86_400_000;
    let curr_start = now - period_ms;
    let prev_start = curr_start - period_ms;

    let sql = format!(
        r#"
        SELECT t.id, t.name, t.category, t.icon, t.color,
               COALESCE(SUM(CASE WHEN v.visit_time >= ? THEN 1 ELSE 0 END), 0) as curr_visits,
               COALESCE(SUM(CASE WHEN v.visit_time >= ? AND v.visit_time < ? THEN 1 ELSE 0 END), 0) as prev_visits
        FROM topics t
        LEFT JOIN url_topics ut ON t.id = ut.topic_id
        LEFT JOIN urls u ON ut.url_id = u.id {}
        LEFT JOIN visits v ON u.id = v.url_id AND v.visit_time >= ?
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
    for b in &hidden_params {
        query_params.push(b.as_ref());
    }
    query_params.push(&prev_start);

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
}
