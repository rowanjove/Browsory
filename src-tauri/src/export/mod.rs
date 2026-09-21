use chrono::{Local, TimeZone};
use rusqlite::Connection;
use rust_xlsxwriter::{Format, Workbook};
use serde::Deserialize;
use std::fs::File;
use std::io::Write;
use tracing::info;

use crate::database::models::HistoryFilter;
use crate::error::{AppError, AppResult};

#[derive(Debug, Deserialize)]
pub struct ExportOptions {
    pub format: String, // "csv" | "xlsx" | "json"
    pub file_path: String,
    pub filter: Option<HistoryFilter>,
    pub visit_ids: Option<Vec<i64>>,
    pub columns: Option<Vec<String>>,
}

#[derive(Debug, serde::Serialize)]
struct ExportRecord {
    pub id: i64,
    pub time: String,
    pub title: String,
    pub url: String,
    pub domain: String,
    pub browser: String,
    pub profile: String,
    pub duration_seconds: i64,
}

pub fn export_history_to_file(conn: &Connection, options: &ExportOptions) -> AppResult<usize> {
    info!(
        "Exporting history to {:?} format={}",
        options.file_path, options.format
    );

    let records = fetch_records_for_export(conn, options)?;
    let count = records.len();

    let enabled_cols = options.columns.clone().unwrap_or_else(|| {
        vec![
            "time".to_string(),
            "title".to_string(),
            "url".to_string(),
            "domain".to_string(),
            "browser".to_string(),
            "profile".to_string(),
            "duration".to_string(),
        ]
    });

    match options.format.to_lowercase().as_str() {
        "csv" => export_csv(&records, &enabled_cols, &options.file_path)?,
        "xlsx" => export_xlsx(&records, &enabled_cols, &options.file_path)?,
        "json" => export_json(&records, &options.file_path)?,
        fmt => {
            return Err(AppError::System(format!(
                "Unsupported export format: {}",
                fmt
            )))
        }
    }

    info!("Export finished: {} records written", count);
    Ok(count)
}

fn fetch_records_for_export(
    conn: &Connection,
    options: &ExportOptions,
) -> AppResult<Vec<ExportRecord>> {
    let mut conditions = Vec::new();
    let mut param_values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    // 1. If explicit visit_ids are specified
    if let Some(ref ids) = options.visit_ids {
        if !ids.is_empty() {
            let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            conditions.push(format!("v.id IN ({})", placeholders));
            for id in ids {
                param_values.push(Box::new(*id));
            }
        }
    } else if let Some(ref filter) = options.filter {
        // Search condition (Hybrid FTS + substring LIKE to guarantee Chinese & multi-word match)
        if let Some(ref raw) = filter.search {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                let tokens: Vec<String> = trimmed
                    .split_whitespace()
                    .filter_map(|t| {
                        let cleaned: String = t
                            .chars()
                            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == '.')
                            .collect();
                        if cleaned.is_empty() {
                            None
                        } else {
                            Some(format!("\"{}\"*", cleaned.replace('"', "\"\"")))
                        }
                    })
                    .collect();

                let fts_query = tokens.join(" ");
                let like_pattern = format!("%{}%", trimmed);

                if !fts_query.is_empty() {
                    conditions.push("(u.id IN (SELECT rowid FROM urls_fts WHERE urls_fts MATCH ?) OR u.title LIKE ? OR u.url LIKE ?)".to_string());
                    param_values.push(Box::new(fts_query));
                    param_values.push(Box::new(like_pattern.clone()));
                    param_values.push(Box::new(like_pattern));
                } else {
                    conditions.push("(u.title LIKE ? OR u.url LIKE ?)".to_string());
                    param_values.push(Box::new(like_pattern.clone()));
                    param_values.push(Box::new(like_pattern));
                }
            }
        }

        // Browser condition
        if let Some(ref browsers) = filter.browsers {
            if !browsers.is_empty() {
                let placeholders = browsers.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                conditions.push(format!("s.browser IN ({})", placeholders));
                for b in browsers {
                    param_values.push(Box::new(b.clone()));
                }
            }
        }

        // Time range condition
        if let Some(st) = filter.start_time {
            conditions.push("v.visit_time >= ?".to_string());
            param_values.push(Box::new(st));
        }
        if let Some(et) = filter.end_time {
            conditions.push("v.visit_time <= ?".to_string());
            param_values.push(Box::new(et));
        }

        // Profiles condition
        if let Some(ref profiles) = filter.profiles {
            if !profiles.is_empty() {
                let placeholders = profiles.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                conditions.push(format!("s.profile IN ({})", placeholders));
                for p in profiles {
                    param_values.push(Box::new(p.clone()));
                }
            }
        }

        // Favorites filter
        if let Some(true) = filter.only_favorites {
            conditions.push("EXISTS (SELECT 1 FROM favorites f WHERE f.url_id = u.id)".to_string());
        }

        // Tag filter
        if let Some(ref tag_name) = filter.tag {
            conditions.push("EXISTS (SELECT 1 FROM url_tags ut JOIN tags t ON ut.tag_id = t.id WHERE ut.url_id = u.id AND t.name = ?)".to_string());
            param_values.push(Box::new(tag_name.clone()));
        }
    }

    // Exclude Hidden privacy rules from export (Fail-closed)
    let rules = crate::database::repository::get_active_privacy_rules(conn)?;
    crate::database::repository::build_hidden_rule_conditions(
        &rules,
        "u.domain",
        "u.url",
        &mut conditions,
        &mut param_values,
    );

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let sort_order = if let Some(ref filter) = options.filter {
        match filter.sort_by.as_deref().unwrap_or("time_desc") {
            "time_asc" => "ORDER BY v.visit_time ASC, v.id ASC",
            "duration" => "ORDER BY v.visit_duration DESC, v.visit_time DESC, v.id DESC",
            _ => "ORDER BY v.visit_time DESC, v.id DESC",
        }
    } else {
        "ORDER BY v.visit_time DESC, v.id DESC"
    };

    let sql = format!(
        r#"
        SELECT v.id, v.visit_time, COALESCE(u.title, '') as title, u.url, COALESCE(u.domain, '') as domain,
               s.browser, s.profile, v.visit_duration
        FROM visits v
        JOIN urls u ON v.url_id = u.id
        JOIN sources s ON v.source_id = s.id
        {}
        {}
        LIMIT 100000
        "#,
        where_clause, sort_order
    );

    let mut stmt = conn.prepare(&sql)?;
    let rusqlite_params: Vec<&dyn rusqlite::ToSql> =
        param_values.iter().map(|b| b.as_ref()).collect();

    let rows = stmt.query_map(rusqlite_params.as_slice(), |row| {
        let id: i64 = row.get(0)?;
        let time_ms: i64 = row.get(1)?;
        let title: String = row.get(2)?;
        let url: String = row.get(3)?;
        let domain: String = row.get(4)?;
        let browser: String = row.get(5)?;
        let profile: String = row.get(6)?;
        let duration: i64 = row.get(7)?;

        let formatted_time = match Local.timestamp_millis_opt(time_ms) {
            chrono::LocalResult::Single(dt) => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
            _ => time_ms.to_string(),
        };

        Ok(ExportRecord {
            id,
            time: formatted_time,
            title,
            url,
            domain,
            browser,
            profile,
            duration_seconds: duration,
        })
    })?;

    let mut list = Vec::new();
    for r in rows {
        list.push(r?);
    }
    Ok(list)
}

fn export_csv(records: &[ExportRecord], cols: &[String], target_path: &str) -> AppResult<()> {
    let mut file = File::create(target_path)?;
    // Write UTF-8 BOM so Microsoft Excel on Windows parses Chinese correctly without garbled text
    file.write_all(b"\xEF\xBB\xBF")?;
    let mut writer = csv::Writer::from_writer(file);

    // Build header based on active columns
    let mut header = Vec::new();
    for col in cols {
        match col.as_str() {
            "time" => header.push("访问时间"),
            "title" => header.push("网页标题"),
            "url" => header.push("URL"),
            "domain" => header.push("域名"),
            "browser" => header.push("浏览器"),
            "profile" => header.push("配置文件"),
            "duration" => header.push("停留时长(秒)"),
            _ => {}
        }
    }
    writer.write_record(&header)?;

    for r in records {
        let mut row = Vec::new();
        for col in cols {
            match col.as_str() {
                "time" => row.push(r.time.clone()),
                "title" => row.push(r.title.clone()),
                "url" => row.push(r.url.clone()),
                "domain" => row.push(r.domain.clone()),
                "browser" => row.push(r.browser.clone()),
                "profile" => row.push(r.profile.clone()),
                "duration" => row.push(r.duration_seconds.to_string()),
                _ => {}
            }
        }
        writer.write_record(&row)?;
    }

    writer.flush()?;
    Ok(())
}

fn export_xlsx(records: &[ExportRecord], cols: &[String], target_path: &str) -> AppResult<()> {
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();

    let header_format = Format::new().set_bold();

    // Headers
    let mut col_idx = 0;
    for col in cols {
        let header_name = match col.as_str() {
            "time" => "访问时间",
            "title" => "网页标题",
            "url" => "URL",
            "domain" => "域名",
            "browser" => "浏览器",
            "profile" => "配置文件",
            "duration" => "停留时长(秒)",
            _ => continue,
        };
        worksheet.write_string_with_format(0, col_idx, header_name, &header_format)?;
        col_idx += 1;
    }

    // Rows
    for (row_idx, r) in records.iter().enumerate() {
        let row_num = (row_idx + 1) as u32;
        let mut c_idx = 0;
        for col in cols {
            match col.as_str() {
                "time" => {
                    worksheet.write_string(row_num, c_idx, &r.time)?;
                    c_idx += 1;
                }
                "title" => {
                    worksheet.write_string(row_num, c_idx, &r.title)?;
                    c_idx += 1;
                }
                "url" => {
                    worksheet.write_string(row_num, c_idx, &r.url)?;
                    c_idx += 1;
                }
                "domain" => {
                    worksheet.write_string(row_num, c_idx, &r.domain)?;
                    c_idx += 1;
                }
                "browser" => {
                    worksheet.write_string(row_num, c_idx, &r.browser)?;
                    c_idx += 1;
                }
                "profile" => {
                    worksheet.write_string(row_num, c_idx, &r.profile)?;
                    c_idx += 1;
                }
                "duration" => {
                    worksheet.write_number(row_num, c_idx, r.duration_seconds as f64)?;
                    c_idx += 1;
                }
                _ => {}
            }
        }
    }

    worksheet.autofit();
    workbook.save(target_path)?;
    Ok(())
}

fn export_json(records: &[ExportRecord], target_path: &str) -> AppResult<()> {
    let mut file = File::create(target_path)?;
    let json_bytes = serde_json::to_vec_pretty(records)?;
    file.write_all(&json_bytes)?;
    Ok(())
}
