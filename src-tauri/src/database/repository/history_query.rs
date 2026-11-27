use super::{build_rule_conditions, get_active_privacy_rules, DataScope};
use crate::database::models::{HistoryFilter, HistoryPageResult, VisitListItem};
use crate::error::{AppError, AppResult};
use chrono::{Local, NaiveDate, TimeZone};
#[cfg(test)]
use rusqlite::params;
use rusqlite::{Connection, OptionalExtension};

fn local_date_boundary(raw: &str, end: bool) -> AppResult<i64> {
    let date = NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .map_err(|_| AppError::Parse(format!("日期格式无效: {raw}")))?;
    let date = if end {
        date.succ_opt()
            .ok_or_else(|| AppError::Parse("日期超出范围".into()))?
    } else {
        date
    };
    let midnight = date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| AppError::Parse("日期超出范围".into()))?;
    let value = Local
        .from_local_datetime(&midnight)
        .earliest()
        .ok_or_else(|| AppError::Parse("此本地日期的零点不存在，请选择其他日期".into()))?
        .timestamp_millis();
    Ok(if end { value - 1 } else { value })
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

fn parse_advanced_search(raw: &str) -> AppResult<AdvancedSearchCriteria> {
    let mut fts_tokens = Vec::new();
    let mut fts_compatible = true;
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
            after_ms = Some(local_date_boundary(val, false)?);
        } else if let Some(val) = part.strip_prefix("before:") {
            before_ms = Some(local_date_boundary(val, true)?);
        } else {
            plain_words.push(part);
            let cleaned: String = part
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == '.')
                .collect();
            if cleaned == part && !cleaned.is_empty() {
                fts_tokens.push(format!("\"{}\"*", cleaned.replace('"', "\"\"")));
            } else {
                // Special punctuation (e.g. `%`, `+`, `/`) would be stripped by FTS sanitization
                // and widen prefix matches incorrectly; fall back to escaped LIKE filtering instead.
                fts_compatible = false;
            }
        }
    }

    if !fts_compatible {
        fts_tokens.clear();
    }

    if matches!((after_ms, before_ms), (Some(a), Some(b)) if a > b) {
        return Err(AppError::Parse("开始日期不能晚于结束日期".into()));
    }
    Ok(AdvancedSearchCriteria {
        fts_tokens,
        like_query: plain_words.join(" "),
        site,
        browser,
        profile,
        title,
        url_fragment,
        after_ms,
        before_ms,
    })
}

pub(crate) type QueryParameters = Vec<Box<dyn rusqlite::ToSql>>;

fn escape_like_literal(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

pub(crate) fn history_conditions(
    conn: &Connection,
    filter: &HistoryFilter,
) -> AppResult<(Vec<String>, QueryParameters)> {
    if matches!((filter.start_time, filter.end_time), (Some(a), Some(b)) if a > b) {
        return Err(AppError::Parse("开始日期不能晚于结束日期".into()));
    }
    let mut conditions = Vec::new();
    let mut param_values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    // 1. Advanced Search parser & conditions
    if let Some(ref raw) = filter.search {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            let criteria = parse_advanced_search(trimmed)?;

            if let Some(site) = criteria.site {
                conditions
                    .push("(u.domain LIKE ? ESCAPE '\\' OR u.url LIKE ? ESCAPE '\\')".to_string());
                let pat = format!("%{}%", escape_like_literal(&site));
                param_values.push(Box::new(pat.clone()));
                param_values.push(Box::new(pat));
            }

            if let Some(br) = criteria.browser {
                conditions.push("s.browser LIKE ? ESCAPE '\\'".to_string());
                param_values.push(Box::new(format!("%{}%", escape_like_literal(&br))));
            }

            if let Some(prof) = criteria.profile {
                conditions.push("s.profile LIKE ? ESCAPE '\\'".to_string());
                param_values.push(Box::new(format!("%{}%", escape_like_literal(&prof))));
            }

            if let Some(t) = criteria.title {
                conditions.push("u.title LIKE ? ESCAPE '\\'".to_string());
                param_values.push(Box::new(format!("%{}%", escape_like_literal(&t))));
            }

            if let Some(u_frag) = criteria.url_fragment {
                conditions.push("u.url LIKE ? ESCAPE '\\'".to_string());
                param_values.push(Box::new(format!("%{}%", escape_like_literal(&u_frag))));
            }

            if let Some(aft) = criteria.after_ms {
                conditions.push("v.visit_time >= ?".to_string());
                param_values.push(Box::new(aft));
            }

            if let Some(bef) = criteria.before_ms {
                conditions.push("v.visit_time <= ?".to_string());
                param_values.push(Box::new(bef));
            }

            match (
                !criteria.fts_tokens.is_empty(),
                !criteria.like_query.is_empty(),
            ) {
                (true, true) => {
                    let fts_query = criteria.fts_tokens.join(" ");
                    let like_pat = format!("%{}%", escape_like_literal(&criteria.like_query));
                    conditions.push(
                        "u.id IN (SELECT rowid FROM urls_fts WHERE urls_fts MATCH ? UNION SELECT id FROM urls WHERE title LIKE ? ESCAPE '\\' OR url LIKE ? ESCAPE '\\')"
                            .to_string(),
                    );
                    param_values.push(Box::new(fts_query));
                    param_values.push(Box::new(like_pat.clone()));
                    param_values.push(Box::new(like_pat));
                }
                (true, false) => {
                    let fts_query = criteria.fts_tokens.join(" ");
                    conditions.push(
                        "u.id IN (SELECT rowid FROM urls_fts WHERE urls_fts MATCH ?)".to_string(),
                    );
                    param_values.push(Box::new(fts_query));
                }
                (false, true) => {
                    let like_pat = format!("%{}%", escape_like_literal(&criteria.like_query));
                    conditions.push(
                        "u.id IN (SELECT id FROM urls WHERE title LIKE ? ESCAPE '\\' OR url LIKE ? ESCAPE '\\')"
                            .to_string(),
                    );
                    param_values.push(Box::new(like_pat.clone()));
                    param_values.push(Box::new(like_pat));
                }
                (false, false) => {}
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

    // 5b. Enforce local Hidden or full AI privacy scope before pagination limits.
    let active_rules = get_active_privacy_rules(conn)?;
    let scope = if filter.ai_scope {
        DataScope::Ai
    } else {
        DataScope::Local
    };
    build_rule_conditions(
        &active_rules,
        scope,
        "u.domain",
        "u.url",
        &mut conditions,
        &mut param_values,
    );

    Ok((conditions, param_values))
}

// Share ordering with exports. Aggregate frequency once instead of counting per visit.
pub(crate) fn history_sort(sort: &str) -> (&'static str, &'static str, &'static str) {
    match sort {
        "visit_count" => ("LEFT JOIN (SELECT url_id, count(*) AS frequency FROM visits GROUP BY url_id) vc ON vc.url_id=u.id", "COALESCE(vc.frequency,0)", "COALESCE(vc.frequency,0) DESC, v.visit_time DESC, v.id DESC"),
        "duration" => ("", "v.visit_duration", "v.visit_duration DESC, v.visit_time DESC, v.id DESC"),
        "time_asc" => ("", "v.visit_time", "v.visit_time ASC, v.id ASC"),
        _ => ("", "v.visit_time", "v.visit_time DESC, v.id DESC"),
    }
}

pub fn query_history(conn: &Connection, filter: &HistoryFilter) -> AppResult<HistoryPageResult> {
    let limit = filter.limit.unwrap_or(100).clamp(1, 500) as usize;
    let (mut conditions, mut values) = history_conditions(conn, filter)?;
    let sort = filter.sort_by.as_deref().unwrap_or("time_desc");
    let (join, sort_value, order) = history_sort(sort);
    if filter.offset.is_none() {
        if let (Some(time), Some(id)) = (filter.cursor_time, filter.cursor_id) {
            if matches!(sort, "duration" | "visit_count") {
                // Legacy clients supplied only time/id. Resolve their actual primary sort key.
                let primary = match filter.cursor_sort_value {
                    Some(value) => value,
                    None => {
                        let sql = if sort == "duration" {
                            "SELECT visit_duration FROM visits WHERE id=?1"
                        } else {
                            "SELECT count(*) FROM visits WHERE url_id=(SELECT url_id FROM visits WHERE id=?1)"
                        };
                        conn.query_row(sql, [id], |r| r.get::<_, i64>(0))
                            .optional()?
                            .ok_or_else(|| AppError::Parse("分页游标已失效，请刷新列表".into()))?
                    }
                };
                conditions.push(format!("({sort_value} < ? OR ({sort_value} = ? AND (v.visit_time < ? OR (v.visit_time = ? AND v.id < ?))))"));
                values.extend([
                    Box::new(primary) as Box<dyn rusqlite::ToSql>,
                    Box::new(primary),
                    Box::new(time),
                    Box::new(time),
                    Box::new(id),
                ]);
            } else {
                let op = if sort == "time_asc" { ">" } else { "<" };
                conditions.push(format!(
                    "(v.visit_time {op} ? OR (v.visit_time = ? AND v.id {op} ?))"
                ));
                values.extend([
                    Box::new(time) as Box<dyn rusqlite::ToSql>,
                    Box::new(time),
                    Box::new(id),
                ]);
            }
        }
    }
    let where_sql = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };
    let offset = if filter.offset.is_some() {
        " OFFSET ?"
    } else {
        ""
    };
    let sql = if matches!(sort, "visit_count" | "duration") {
        // Defer fetching wide URL/title/domain strings and favorites checks until
        // after sorting and slicing to LIMIT + 1.
        format!(
            "SELECT v.id,v.url_id,v.visit_time,COALESCE(u.title,''),u.url,COALESCE(u.domain,''),s.browser,s.profile,v.visit_duration,v.transition,EXISTS(SELECT 1 FROM favorites f WHERE f.url_id=u.id),ranked.sort_val \
             FROM (SELECT v.id AS visit_id, {sort_value} AS sort_val FROM visits v JOIN urls u ON v.url_id=u.id JOIN sources s ON v.source_id=s.id {join} {where_sql} ORDER BY {order} LIMIT ?{offset}) ranked \
             JOIN visits v ON v.id=ranked.visit_id \
             JOIN urls u ON v.url_id=u.id \
             JOIN sources s ON v.source_id=s.id \
             ORDER BY ranked.sort_val DESC, v.visit_time DESC, v.id DESC"
        )
    } else {
        format!(
            "SELECT v.id,v.url_id,v.visit_time,COALESCE(u.title,''),u.url,COALESCE(u.domain,''),s.browser,s.profile,v.visit_duration,v.transition,EXISTS(SELECT 1 FROM favorites f WHERE f.url_id=u.id),{sort_value} FROM visits v JOIN urls u ON v.url_id=u.id JOIN sources s ON v.source_id=s.id {join} {where_sql} ORDER BY {order} LIMIT ?{offset}"
        )
    };
    values.push(Box::new((limit + 1) as i64));
    if let Some(offset) = filter.offset {
        values.push(Box::new(offset as i64));
    }
    let args: Vec<&dyn rusqlite::ToSql> = values.iter().map(|v| v.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(args.as_slice(), |r| {
        Ok((
            VisitListItem {
                id: r.get(0)?,
                url_id: r.get(1)?,
                visit_time: r.get(2)?,
                title: r.get(3)?,
                url: r.get(4)?,
                domain: r.get(5)?,
                browser: r.get(6)?,
                profile: r.get(7)?,
                visit_duration: r.get(8)?,
                transition: r.get(9)?,
                is_favorite: r.get::<_, i64>(10)? != 0,
            },
            r.get::<_, i64>(11)?,
        ))
    })?;
    let mut records = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    let has_more = records.len() > limit;
    records.truncate(limit);
    let last = if has_more { records.last() } else { None };
    let (next_cursor_time, next_cursor_id, next_cursor_sort_value) = last
        .map(|(r, value)| (Some(r.visit_time), Some(r.id), Some(*value)))
        .unwrap_or((None, None, None));
    Ok(HistoryPageResult {
        items: records.into_iter().map(|(r, _)| r).collect(),
        next_cursor_time,
        next_cursor_id,
        next_cursor_sort_value,
        has_more,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::{export_history_to_file, export_history_to_file_checked, ExportOptions};
    use std::fs::File;
    use std::io::Read;

    fn fixture() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::database::migrations::run_migrations(&mut conn).unwrap();
        conn.execute("INSERT INTO sources(id,browser,profile,source_type,history_path) VALUES(1,'chrome','Work','sqlite','synthetic')",[]).unwrap();
        for (id, url, title) in [
            (1, "https://example.com/a", "=1+2"),
            (2, "https://example.com/b", "Rust Guide"),
            (3, "https://other.test/c", "Other"),
        ] {
            conn.execute(
                "INSERT INTO urls(id,url,title,domain) VALUES(?1,?2,?3,?4)",
                params![
                    id,
                    url,
                    title,
                    if id == 3 { "other.test" } else { "example.com" }
                ],
            )
            .unwrap();
        }
        for (id, url, time, duration) in [
            (1, 1, 100, 1000),
            (2, 2, 300, 900),
            (3, 2, 200, 800),
            (4, 3, 400, 700),
            (5, 1, 100, 1000),
            (6, 2, 200, 800),
        ] {
            conn.execute("INSERT INTO visits(id,source_id,url_id,source_visit_id,visit_time,visit_duration,event_hash,imported_at) VALUES(?1,1,?2,?1,?3,?4,?5,0)",params![id,url,time,duration,format!("fixture-{id}")]).unwrap();
        }
        conn
    }
    #[test]
    fn every_sort_pages_without_gaps_duplicates_or_tie_errors() {
        let conn = fixture();
        for (sort, expected) in [
            ("time_desc", vec![4, 2, 6, 3, 5, 1]),
            ("time_asc", vec![1, 5, 3, 6, 2, 4]),
            ("duration", vec![5, 1, 2, 6, 3, 4]),
            ("visit_count", vec![2, 6, 3, 5, 1, 4]),
        ] {
            let mut filter = HistoryFilter {
                sort_by: Some(sort.into()),
                limit: Some(2),
                ..Default::default()
            };
            let mut ids = vec![];
            loop {
                let page = query_history(&conn, &filter).unwrap();
                ids.extend(page.items.iter().map(|r| r.id));
                if !page.has_more {
                    break;
                }
                filter.cursor_time = page.next_cursor_time;
                filter.cursor_id = page.next_cursor_id;
                filter.cursor_sort_value = page.next_cursor_sort_value;
                assert!(ids.len() < 10, "pagination did not progress");
            }
            assert_eq!(ids, expected, "{sort}");
        }
        for sort in ["duration", "visit_count"] {
            let mut f = HistoryFilter {
                sort_by: Some(sort.into()),
                limit: Some(2),
                ..Default::default()
            };
            let first = query_history(&conn, &f).unwrap();
            f.cursor_time = first.next_cursor_time;
            f.cursor_id = first.next_cursor_id;
            let legacy = query_history(&conn, &f).unwrap();
            f.cursor_sort_value = first.next_cursor_sort_value;
            assert_eq!(
                legacy.items.iter().map(|i| i.id).collect::<Vec<_>>(),
                query_history(&conn, &f)
                    .unwrap()
                    .items
                    .iter()
                    .map(|i| i.id)
                    .collect::<Vec<_>>()
            );
        }
    }
    #[test]
    fn local_dates_include_last_millisecond_and_reject_invalid_ranges() {
        let start = local_date_boundary("2026-11-17", false).unwrap();
        let end = local_date_boundary("2026-11-17", true).unwrap();
        assert_eq!((end + 1) % 1000, 0);
        assert!(end > start);
        let parsed = parse_advanced_search("after:2026-11-17 before:2026-11-17").unwrap();
        assert_eq!(parsed.after_ms, Some(start));
        assert_eq!(parsed.before_ms, Some(end));
        assert!(parse_advanced_search("after:invalid").is_err());
        assert!(parse_advanced_search("after:2026-11-18 before:2026-11-17").is_err());
    }
    #[test]
    fn export_matches_advanced_filters_privacy_and_selected_columns() {
        let conn = fixture();
        conn.execute("INSERT INTO privacy_rules(pattern,rule_type,enabled,created_at) VALUES('other.test','hidden',1,0)",[]).unwrap();
        let dir =
            std::env::temp_dir().join(format!("browsory_export_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("export.json");
        for search in [
            "site:example.com",
            "browser:chrome profile:Work",
            "title:Rust",
            "url:/a",
        ] {
            let filter = HistoryFilter {
                search: Some(search.into()),
                sort_by: Some("duration".into()),
                ..Default::default()
            };
            let expected = query_history(&conn, &filter).unwrap().items.len();
            let options = ExportOptions {
                format: "json".into(),
                file_path: path.to_string_lossy().into(),
                filter: Some(filter),
                visit_ids: None,
                columns: Some(vec!["title".into()]),
            };
            assert_eq!(export_history_to_file(&conn, &options).unwrap(), expected);
            let data: Vec<serde_json::Value> =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!(data.len(), expected);
            assert!(data
                .iter()
                .all(|r| r.as_object().unwrap().len() == 1 && r.get("title").is_some()));
        }
        let options = ExportOptions {
            format: "csv".into(),
            file_path: dir.join("export.csv").to_string_lossy().into(),
            filter: None,
            visit_ids: Some(vec![1, 4]),
            columns: Some(vec!["title".into()]),
        };
        assert_eq!(export_history_to_file(&conn, &options).unwrap(), 1);
        assert!(std::fs::read_to_string(&options.file_path)
            .unwrap()
            .contains("'=1+2"));
        let empty = ExportOptions {
            visit_ids: Some(vec![]),
            ..options
        };
        assert_eq!(export_history_to_file(&conn, &empty).unwrap(), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn exports_over_one_hundred_thousand_are_complete_in_all_formats() {
        let conn = fixture();
        conn.execute_batch("WITH RECURSIVE n(x) AS (VALUES(7) UNION ALL SELECT x+1 FROM n WHERE x<100005) INSERT INTO visits(id,source_id,url_id,source_visit_id,visit_time,visit_duration,event_hash,imported_at) SELECT x,1,2,x,x,1,'bulk-'||x,0 FROM n;").unwrap();
        let dir =
            std::env::temp_dir().join(format!("browsory_export_large_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        for format in ["csv", "json", "xlsx"] {
            let path = dir.join(format!("export.{format}"));
            let options = ExportOptions {
                format: format.into(),
                file_path: path.to_string_lossy().into(),
                filter: None,
                visit_ids: None,
                columns: Some(vec!["url".into()]),
            };
            assert_eq!(export_history_to_file(&conn, &options).unwrap(), 100005);
            if format == "csv" {
                assert_eq!(
                    csv::Reader::from_path(&path).unwrap().records().count(),
                    100005
                );
            } else if format == "json" {
                let rows: Vec<serde_json::Value> =
                    serde_json::from_reader(File::open(&path).unwrap()).unwrap();
                assert_eq!(rows.len(), 100005);
            } else {
                let mut zip = zip::ZipArchive::new(File::open(&path).unwrap()).unwrap();
                let mut xml = String::new();
                zip.by_name("xl/worksheets/sheet1.xml")
                    .unwrap()
                    .read_to_string(&mut xml)
                    .unwrap();
                assert_eq!(xml.matches("<row ").count(), 100006);
            }
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn failed_or_cancelled_export_preserves_existing_destination() {
        let conn = fixture();
        let dir =
            std::env::temp_dir().join(format!("browsory_export_cancel_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("keep.csv");
        std::fs::write(&path, b"original").unwrap();
        let options = ExportOptions {
            format: "csv".into(),
            file_path: path.to_string_lossy().into(),
            filter: None,
            visit_ids: None,
            columns: None,
        };
        let calls = std::sync::atomic::AtomicUsize::new(0);
        assert!(export_history_to_file_checked(&conn, &options, || {
            if calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1 >= 3 {
                Err(AppError::VaultLocked("cancel fixture".into()))
            } else {
                Ok(())
            }
        })
        .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn successful_exports_replace_existing_destinations_in_every_format() {
        let conn = fixture();
        let dir =
            std::env::temp_dir().join(format!("browsory_export_replace_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        for format in ["csv", "json", "xlsx"] {
            let path = dir.join(format!("existing.{format}"));
            std::fs::write(&path, b"original").unwrap();
            let count = export_history_to_file(
                &conn,
                &ExportOptions {
                    format: format.into(),
                    file_path: path.to_string_lossy().into_owned(),
                    filter: None,
                    visit_ids: None,
                    columns: None,
                },
            )
            .unwrap();
            assert!(count > 0);
            assert_ne!(std::fs::read(path).unwrap(), b"original");
        }
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 3);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn search_retains_substring_and_cjk_matches_alongside_fts_hits_and_escapes_wildcards() {
        let conn = fixture();
        conn.execute(
            "INSERT INTO urls(id,url,title,domain) VALUES(10,'https://noise.test/hub','Hub Noise','noise.test')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO urls(id,url,title,domain) VALUES(11,'https://github.com/rust-lang/rust','GitHub Repository','github.com')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO urls(id,url,title,domain) VALUES(12,'https://notes.test/browser','浏览器历史归档工具','notes.test')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO urls(id,url,title,domain) VALUES(13,'https://literal.test/100_percent','Literal 100% _test_','literal.test')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO urls(id,url,title,domain) VALUES(14,'https://wildcard.test/100Xpercent','Literal 100X XtestX','wildcard.test')",
            [],
        )
        .unwrap();
        for (vid, uid, t) in [
            (10, 10, 500),
            (11, 11, 600),
            (12, 12, 700),
            (13, 13, 800),
            (14, 14, 900),
        ] {
            conn.execute(
                "INSERT INTO visits(id,source_id,url_id,source_visit_id,visit_time,visit_duration,event_hash,imported_at) VALUES(?1,1,?2,?1,?3,10,?4,0)",
                params![vid, uid, t, format!("sub-{vid}")],
            )
            .unwrap();
        }

        // 'hub' hits FTS on id=10 ('Hub Noise'), and must still retain substring hit id=11 ('github.com')
        let hub_results = query_history(
            &conn,
            &HistoryFilter {
                search: Some("hub".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let hub_urls: Vec<&str> = hub_results.items.iter().map(|i| i.url.as_str()).collect();
        assert!(hub_urls.contains(&"https://noise.test/hub"));
        assert!(hub_urls.contains(&"https://github.com/rust-lang/rust"));

        // CJK multi-character substring search must match via LIKE alongside FTS
        let cjk_results = query_history(
            &conn,
            &HistoryFilter {
                search: Some("历史归档".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(cjk_results.items.len(), 1);
        assert_eq!(cjk_results.items[0].url, "https://notes.test/browser");

        // Literal '%' and '_' in search/title:/url:/site: must not act as wildcards
        for query in ["title:100%", "url:100_percent", "100%"] {
            let escaped = query_history(
                &conn,
                &HistoryFilter {
                    search: Some(query.into()),
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(
                escaped.items.len(),
                1,
                "query `{query}` should match only literal item"
            );
            assert_eq!(escaped.items[0].url, "https://literal.test/100_percent");
        }
    }
}
