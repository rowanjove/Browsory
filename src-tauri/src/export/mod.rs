use chrono::{Local, TimeZone};
use rusqlite::Connection;
use rust_xlsxwriter::{Format, Workbook, Worksheet};
use serde::Deserialize;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::Path;

use crate::database::models::HistoryFilter;
use crate::database::repository::history_query::{history_conditions, history_sort};
use crate::error::{AppError, AppResult};

#[derive(Debug, Deserialize)]
pub struct ExportOptions {
    pub format: String,
    pub file_path: String,
    pub filter: Option<HistoryFilter>,
    pub visit_ids: Option<Vec<i64>>,
    pub columns: Option<Vec<String>>,
}

#[derive(Debug, serde::Serialize)]
struct ExportRecord {
    id: i64,
    time: String,
    title: String,
    url: String,
    domain: String,
    browser: String,
    profile: String,
    duration_seconds: i64,
}
const COLUMNS: &[&str] = &[
    "time", "title", "url", "domain", "browser", "profile", "duration",
];

impl ExportRecord {
    fn value(&self, col: &str) -> String {
        match col {
            "time" => self.time.clone(),
            "title" => self.title.clone(),
            "url" => self.url.clone(),
            "domain" => self.domain.clone(),
            "browser" => self.browser.clone(),
            "profile" => self.profile.clone(),
            "duration" => self.duration_seconds.to_string(),
            _ => String::new(),
        }
    }
}
fn label(col: &str) -> &str {
    match col {
        "time" => "访问时间",
        "title" => "网页标题",
        "url" => "URL",
        "domain" => "域名",
        "browser" => "浏览器",
        "profile" => "配置文件",
        "duration" => "停留时长(秒)",
        _ => col,
    }
}

pub fn export_history_to_file(conn: &Connection, options: &ExportOptions) -> AppResult<usize> {
    export_history_to_file_checked(conn, options, || Ok(()))
}

/// Explicit user exports may be plaintext. Stream to an adjacent staging file;
/// failed/cancelled output never replaces an existing destination.
pub fn export_history_to_file_checked(
    conn: &Connection,
    options: &ExportOptions,
    check: impl Fn() -> AppResult<()> + Sync,
) -> AppResult<usize> {
    check()?;
    let format = options.format.to_lowercase();
    if !matches!(format.as_str(), "csv" | "json" | "xlsx") {
        return Err(AppError::Parse("不支持的导出格式".into()));
    }
    let columns = options
        .columns
        .clone()
        .unwrap_or_else(|| COLUMNS.iter().map(|s| s.to_string()).collect());
    if columns.is_empty() || columns.iter().any(|s| !COLUMNS.contains(&s.as_str())) {
        return Err(AppError::Parse("请选择有效的导出列".into()));
    }
    let target = Path::new(&options.file_path);
    if let Some(database) = conn.path().filter(|p| !p.is_empty()) {
        if target
            .canonicalize()
            .ok()
            .zip(Path::new(database).canonicalize().ok())
            .is_some_and(|(a, b)| a == b)
        {
            return Err(AppError::Security(
                "不能用导出文件覆盖正在使用的历史数据库".into(),
            ));
        }
    }
    let parent = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let stage = parent.join(format!(".browsory-export-{}.tmp", uuid::Uuid::new_v4()));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&stage)?;
    let result = (|| {
        let count = match format.as_str() {
            "csv" => write_csv(conn, options, &columns, file, &check)?,
            "json" => write_json(conn, options, &columns, file, &check)?,
            _ => write_xlsx(conn, options, &columns, file, &check)?,
        };
        check()?;
        OpenOptions::new().write(true).open(&stage)?.sync_all()?;
        fs::rename(&stage, target)?;
        Ok(count)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&stage);
    }
    result
}

fn visit_records(
    conn: &Connection,
    options: &ExportOptions,
    check: &impl Fn() -> AppResult<()>,
    mut visit: impl FnMut(ExportRecord) -> AppResult<()>,
) -> AppResult<usize> {
    let empty = HistoryFilter::default();
    let filter = if options.visit_ids.is_some() {
        &empty
    } else {
        options.filter.as_ref().unwrap_or(&empty)
    };
    let (mut conditions, mut values) = history_conditions(conn, filter)?;
    if let Some(ids) = &options.visit_ids {
        // A single JSON parameter avoids SQLite's bound-variable limit. Empty means empty, never all.
        conditions.push("v.id IN (SELECT CAST(value AS INTEGER) FROM json_each(?))".into());
        values.push(Box::new(serde_json::to_string(ids)?));
    }
    let sort = options
        .filter
        .as_ref()
        .and_then(|f| f.sort_by.as_deref())
        .unwrap_or("time_desc");
    let (join, _, order) = history_sort(sort);
    let where_sql = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };
    let sql=format!("SELECT v.id,v.visit_time,COALESCE(u.title,''),u.url,COALESCE(u.domain,''),s.browser,s.profile,v.visit_duration FROM visits v JOIN urls u ON v.url_id=u.id JOIN sources s ON v.source_id=s.id {join} {where_sql} ORDER BY {order}");
    let args: Vec<&dyn rusqlite::ToSql> = values.iter().map(|v| v.as_ref()).collect();
    let mut statement = conn.prepare(&sql)?;
    let mut rows = statement.query(args.as_slice())?;
    let mut count = 0;
    while let Some(row) = rows.next()? {
        if count % 128 == 0 {
            check()?;
        }
        let time_ms: i64 = row.get(1)?;
        let time = Local
            .timestamp_millis_opt(time_ms)
            .single()
            .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| time_ms.to_string());
        visit(ExportRecord {
            id: row.get(0)?,
            time,
            title: row.get(2)?,
            url: row.get(3)?,
            domain: row.get(4)?,
            browser: row.get(5)?,
            profile: row.get(6)?,
            duration_seconds: row.get(7)?,
        })?;
        count += 1;
    }
    check()?;
    Ok(count)
}

fn write_csv(
    conn: &Connection,
    options: &ExportOptions,
    columns: &[String],
    mut file: File,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<usize> {
    file.write_all(b"\xEF\xBB\xBF")?;
    let mut writer = csv::Writer::from_writer(BufWriter::new(file));
    writer.write_record(columns.iter().map(|c| label(c)))?;
    let count = visit_records(conn, options, check, |record| {
        writer.write_record(columns.iter().map(|col| {
            let value = record.value(col);
            // Titles and URLs are untrusted text. Prevent spreadsheet formula execution.
            if col != "duration"
                && value
                    .trim_start()
                    .starts_with(['=', '+', '-', '@', '\t', '\r', '\n'])
            {
                format!("'{value}")
            } else {
                value
            }
        }))?;
        Ok(())
    })?;
    writer.flush()?;
    Ok(count)
}
fn write_json(
    conn: &Connection,
    options: &ExportOptions,
    columns: &[String],
    file: File,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<usize> {
    let mut writer = BufWriter::new(file);
    writer.write_all(b"[\n")?;
    let mut first = true;
    let count = visit_records(conn, options, check, |record| {
        if !first {
            writer.write_all(b",\n")?;
        }
        first = false;
        if options.columns.is_none() {
            serde_json::to_writer(&mut writer, &record)?;
        } else {
            let mut object = serde_json::Map::new();
            for col in columns {
                let (key, value) = if col == "duration" {
                    (
                        "duration_seconds".to_string(),
                        serde_json::json!(record.duration_seconds),
                    )
                } else {
                    (col.clone(), serde_json::Value::String(record.value(col)))
                };
                object.insert(key, value);
            }
            serde_json::to_writer(&mut writer, &object)?;
        }
        Ok(())
    })?;
    writer.write_all(b"\n]\n")?;
    writer.flush()?;
    Ok(count)
}
fn prepare_sheet(sheet: &mut Worksheet, columns: &[String]) -> AppResult<()> {
    let format = Format::new().set_bold();
    sheet.set_freeze_panes(1, 0)?;
    for (i, col) in columns.iter().enumerate() {
        sheet.write_string_with_format(0, i as u16, label(col), &format)?;
        sheet.set_column_width(
            i as u16,
            if col == "title" || col == "url" {
                55.0
            } else {
                22.0
            },
        )?;
    }
    Ok(())
}
fn write_xlsx(
    conn: &Connection,
    options: &ExportOptions,
    columns: &[String],
    file: File,
    check: &(impl Fn() -> AppResult<()> + Sync),
) -> AppResult<usize> {
    let mut workbook = Workbook::new();
    prepare_sheet(workbook.add_worksheet_with_constant_memory()?, columns)?;
    let mut position = 0usize;
    const ROWS_PER_SHEET: usize = 1_048_575;
    let count = visit_records(conn, options, check, |record| {
        let sheet_index = position / ROWS_PER_SHEET;
        let row = (position % ROWS_PER_SHEET + 1) as u32;
        if position > 0 && position.is_multiple_of(ROWS_PER_SHEET) {
            prepare_sheet(workbook.add_worksheet_with_constant_memory()?, columns)?;
        }
        let sheet = workbook.worksheet_from_index(sheet_index)?;
        for (i, col) in columns.iter().enumerate() {
            if col == "duration" {
                sheet.write_number(row, i as u16, record.duration_seconds as f64)?;
            } else {
                sheet.write_string(row, i as u16, record.value(col))?;
            }
        }
        position += 1;
        Ok(())
    })?;
    check()?;
    workbook.save_to_writer(CheckedWriter { file, check })?;
    Ok(count)
}

/// ZIP serialization can outlive the SQL scan. Check cancellation on writes
/// too, so locking need not wait for an entire workbook to finish packing.
struct CheckedWriter<'a, F> {
    file: File,
    check: &'a F,
}

impl<F: Fn() -> AppResult<()> + Sync> Write for CheckedWriter<'_, F> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        (self.check)().map_err(|error| std::io::Error::other(error.to_string()))?;
        self.file.write(bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        (self.check)().map_err(|error| std::io::Error::other(error.to_string()))?;
        self.file.flush()
    }
}
