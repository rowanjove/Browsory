use tauri::State;

use crate::database::models::{
    AnalyticsSummary, DomainDetail, DomainDynamics, HistoryFilter, HistoryPageResult,
    LinkHealthStatus, OnThisDayResult, ResearchSession, ResumeSuggestion, SmartCollection,
    TakeoutImportSummary, VisitDetail, WebMemoryAnswer, WebsiteRankingItem,
};
use crate::database::repository::{
    create_smart_collection as repo_create_smart_collection,
    delete_smart_collection as repo_delete_smart_collection, delete_visits as repo_delete_visits,
    get_analytics_summary as repo_get_analytics_summary,
    get_domain_detail as repo_get_domain_detail, get_domain_dynamics as repo_get_domain_dynamics,
    get_on_this_day as repo_get_on_this_day, get_research_sessions as repo_get_research_sessions,
    get_research_sessions_for_ai as repo_get_research_sessions_for_ai,
    get_visit_detail as repo_get_visit_detail, get_website_ranking as repo_get_website_ranking,
    list_smart_collections as repo_list_smart_collections, query_history, query_history_for_ai,
};
use crate::database::DbState;
use crate::error::AppResult;

#[tauri::command]
pub fn get_history_page(
    filter: HistoryFilter,
    db: State<'_, DbState>,
) -> AppResult<HistoryPageResult> {
    let conn = db.conn.lock().unwrap();
    query_history(&conn, &filter)
}

#[tauri::command]
pub fn get_visit_detail(visit_id: i64, db: State<'_, DbState>) -> AppResult<Option<VisitDetail>> {
    let conn = db.conn.lock().unwrap();
    repo_get_visit_detail(&conn, visit_id)
}

#[tauri::command]
pub fn delete_visits(visit_ids: Vec<i64>, db: State<'_, DbState>) -> AppResult<u32> {
    let conn = db.conn.lock().unwrap();
    repo_delete_visits(&conn, &visit_ids)
}

#[tauri::command]
pub fn get_analytics(
    days: Option<u32>,
    start_time: Option<i64>,
    end_time: Option<i64>,
    db: State<'_, DbState>,
) -> AppResult<AnalyticsSummary> {
    let conn = db.conn.lock().unwrap();
    repo_get_analytics_summary(&conn, start_time, end_time, days)
}

#[tauri::command]
pub fn export_history(
    options: crate::export::ExportOptions,
    db: State<'_, DbState>,
) -> AppResult<usize> {
    let conn = db.conn.lock().unwrap();
    crate::export::export_history_to_file(&conn, &options)
}

#[tauri::command]
pub fn toggle_favorite(url_id: i64, db: State<'_, DbState>) -> AppResult<bool> {
    let conn = db.conn.lock().unwrap();
    crate::database::repository::toggle_favorite(&conn, url_id)
}

#[tauri::command]
pub fn list_tags(db: State<'_, DbState>) -> AppResult<Vec<crate::database::models::TagItem>> {
    let conn = db.conn.lock().unwrap();
    crate::database::repository::list_tags(&conn)
}

#[tauri::command]
pub fn add_tag_to_url(url_id: i64, tag_name: String, db: State<'_, DbState>) -> AppResult<()> {
    let conn = db.conn.lock().unwrap();
    crate::database::repository::add_tag_to_url(&conn, url_id, &tag_name)
}

#[tauri::command]
pub fn remove_tag_from_url(url_id: i64, tag_name: String, db: State<'_, DbState>) -> AppResult<()> {
    let conn = db.conn.lock().unwrap();
    crate::database::repository::remove_tag_from_url(&conn, url_id, &tag_name)
}

#[tauri::command]
pub fn get_research_sessions(
    days: Option<u32>,
    limit: Option<u32>,
    db: State<'_, DbState>,
) -> AppResult<Vec<ResearchSession>> {
    let conn = db.conn.lock().unwrap();
    repo_get_research_sessions(&conn, days, limit.unwrap_or(50))
}

#[tauri::command]
pub fn get_website_ranking(
    start_time: Option<i64>,
    end_time: Option<i64>,
    limit: Option<u32>,
    db: State<'_, DbState>,
) -> AppResult<Vec<WebsiteRankingItem>> {
    let conn = db.conn.lock().unwrap();
    repo_get_website_ranking(&conn, start_time, end_time, limit)
}

#[tauri::command]
pub fn get_domain_detail(
    domain: String,
    start_time: Option<i64>,
    end_time: Option<i64>,
    db: State<'_, DbState>,
) -> AppResult<DomainDetail> {
    let conn = db.conn.lock().unwrap();
    repo_get_domain_detail(&conn, &domain, start_time, end_time)
}

#[tauri::command]
pub fn get_domain_dynamics(
    start_time: Option<i64>,
    end_time: Option<i64>,
    db: State<'_, DbState>,
) -> AppResult<DomainDynamics> {
    let conn = db.conn.lock().unwrap();
    repo_get_domain_dynamics(&conn, start_time, end_time)
}

#[tauri::command]
pub fn get_on_this_day(
    target_date: Option<String>,
    year: Option<i32>,
    page: Option<u32>,
    page_size: Option<u32>,
    db: State<'_, DbState>,
) -> AppResult<OnThisDayResult> {
    let conn = db.conn.lock().unwrap();
    repo_get_on_this_day(&conn, target_date, year, page, page_size)
}

#[tauri::command]
pub fn list_smart_collections(db: State<'_, DbState>) -> AppResult<Vec<SmartCollection>> {
    let conn = db.conn.lock().unwrap();
    repo_list_smart_collections(&conn)
}

#[tauri::command]
pub fn create_smart_collection(
    name: String,
    query: String,
    filter_json: Option<String>,
    db: State<'_, DbState>,
) -> AppResult<SmartCollection> {
    let conn = db.conn.lock().unwrap();
    repo_create_smart_collection(&conn, &name, &query, filter_json.as_deref())
}

#[tauri::command]
pub fn delete_smart_collection(id: i64, db: State<'_, DbState>) -> AppResult<()> {
    let conn = db.conn.lock().unwrap();
    repo_delete_smart_collection(&conn, id)
}

#[tauri::command]
pub async fn ask_web_memory(
    question: String,
    db: State<'_, DbState>,
) -> Result<WebMemoryAnswer, String> {
    let citations = {
        let conn = db.conn.lock().unwrap();
        query_history_for_ai(&conn, &question, 20).map_err(|e| e.to_string())?
    };

    if citations.is_empty() {
        return Ok(WebMemoryAnswer {
            answer: "在您的个人 Web 历史归档中，未能检索到与该问题直接相关的记录。请尝试更换搜索词，或确认相关历史是否已被同步/过滤。".to_string(),
            citations: Vec::new(),
        });
    }

    let mut context_lines = Vec::new();
    for (idx, c) in citations.iter().enumerate() {
        let time_str = chrono::DateTime::from_timestamp_millis(c.visit_time)
            .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_default();
        context_lines.push(format!(
            "[{}] 标题: {} | 域名: {} | 时间: {} | 链接: {}",
            idx + 1,
            c.title,
            c.domain,
            time_str,
            c.url
        ));
    }
    let context_text = context_lines.join("\n");

    let system_prompt = "你是一个专业严谨的 Local-First 个人 Web 记忆助手。用户正在查询其个人的网页浏览历史记录。\n\
        请结合提供的历史访问上下文直接回答用户的问题。\n\
        回答要求：\n\
        1. 必须基于提供的真实历史记录，客观总结与回答，不要编造未出现过的事实；\n\
        2. 回答中如果引用了某条记录，请标注引用序号如 [1]、[2]；\n\
        3. 语言保持中文、简洁干练、结构清晰。";

    let prompt = format!(
        "用户提问：{}\n\n以下是从用户本地浏览器历史中检索出的相关候选网页证据：\n{}\n\n请结合以上历史证据回答用户的问题：",
        question, context_text
    );

    let answer = crate::commands::settings::call_ai_completion(
        crate::commands::settings::AiRequestPayload {
            prompt,
            system_prompt: Some(system_prompt.to_string()),
        },
        db,
    )
    .await?;

    Ok(WebMemoryAnswer { answer, citations })
}

#[tauri::command]
pub async fn get_resume_suggestion(db: State<'_, DbState>) -> Result<ResumeSuggestion, String> {
    let latest_session = {
        let conn = db.conn.lock().unwrap();
        let sessions =
            repo_get_research_sessions_for_ai(&conn, Some(7), 1).map_err(|e| e.to_string())?;
        sessions.into_iter().next()
    };

    let session = match latest_session {
        Some(s) => s,
        None => {
            return Err("近期暂无连续的研究会话记录，无法生成续研建议".to_string());
        }
    };

    let start_time_str = chrono::DateTime::from_timestamp_millis(session.start_time)
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default();

    let duration_mins = (session.duration_secs / 60).max(1);

    let system_prompt = "你是一个高效的个人研究助理与认知续航助手。\n\
        用户提供其最近一次连续研究探索会话（Research Session）的概要信息。\n\
        请给出精准的【断点续研建议】，包含两部分：\n\
        1. 简要说明用户当时在研究/解决什么核心议题（1-2句话）；\n\
        2. 列出 3 点最具操作性的下一步续研行动建议（每点以 - 开头）。\n\
        输出格式请保持简洁规范，无需多余寒暄。";

    let prompt = format!(
        "最近会话时间：{}\n持续时长：约 {} 分钟\n访问页面数：{} 个\n核心主导域名：{}\n代表性网页标题：{}\n浏览器：{}\n\n请为用户生成断点续研与下一步探索建议：",
        start_time_str,
        duration_mins,
        session.visit_count,
        session.dominant_domain,
        session.sample_title,
        session.browser
    );

    let ai_text = crate::commands::settings::call_ai_completion(
        crate::commands::settings::AiRequestPayload {
            prompt,
            system_prompt: Some(system_prompt.to_string()),
        },
        db,
    )
    .await?;

    let mut next_steps = Vec::new();
    let mut summary_lines = Vec::new();

    for line in ai_text.lines() {
        let trimmed = line.trim();
        let is_numbered = trimmed.chars().next().is_some_and(|c| c.is_ascii_digit())
            && trimmed.chars().nth(1) == Some('.');

        if trimmed.starts_with('-') || trimmed.starts_with('*') || is_numbered {
            next_steps.push(
                trimmed
                    .trim_start_matches(|c: char| {
                        c == '-' || c == '*' || c.is_ascii_digit() || c == '.' || c == ' '
                    })
                    .to_string(),
            );
        } else if !trimmed.is_empty() {
            summary_lines.push(trimmed);
        }
    }

    let summary = if summary_lines.is_empty() {
        format!(
            "您在 {} 集中研读了关于 {} 的内容（{} 个页面）。",
            start_time_str, session.dominant_domain, session.visit_count
        )
    } else {
        summary_lines.join("\n")
    };

    if next_steps.is_empty() {
        next_steps.push(format!(
            "继续深入阅读 {} 相关文档与资源",
            session.dominant_domain
        ));
        next_steps.push("回顾该会话中停留时间最长的重要页面".to_string());
        next_steps.push("整理核心结论或将其保存至个人笔记".to_string());
    }

    Ok(ResumeSuggestion {
        session,
        summary,
        next_steps,
    })
}

#[tauri::command]
pub async fn import_takeout_file(
    file_path: String,
    db: State<'_, DbState>,
) -> Result<TakeoutImportSummary, String> {
    let mut conn = db.conn.lock().unwrap();
    crate::history::takeout::import_google_takeout_file(&mut conn, std::path::Path::new(&file_path))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn check_link_health(
    url_id: i64,
    db: State<'_, DbState>,
) -> Result<LinkHealthStatus, String> {
    let url: String = {
        let conn = db.conn.lock().unwrap();
        conn.query_row(
            "SELECT url FROM urls WHERE id = ?1",
            rusqlite::params![url_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?
    };

    let (status_code, is_alive, error_message) =
        crate::history::health::probe_url_status(&url).await;
    let now = chrono::Utc::now().timestamp_millis();
    let wayback_url = crate::history::health::build_wayback_url(&url);

    {
        let conn = db.conn.lock().unwrap();
        conn.execute(
            r#"
            INSERT INTO link_health (url_id, status_code, is_alive, last_checked_at, error_message)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(url_id) DO UPDATE SET
                status_code = excluded.status_code,
                is_alive = excluded.is_alive,
                last_checked_at = excluded.last_checked_at,
                error_message = excluded.error_message
            "#,
            rusqlite::params![
                url_id,
                status_code.map(|c| c as i64),
                if is_alive { 1 } else { 0 },
                now,
                error_message
            ],
        )
        .map_err(|e| e.to_string())?;
    }

    Ok(LinkHealthStatus {
        url_id,
        url,
        status_code,
        is_alive,
        last_checked_at: now,
        error_message,
        wayback_url,
    })
}

#[tauri::command]
pub fn get_link_health(
    url_id: i64,
    db: State<'_, DbState>,
) -> Result<Option<LinkHealthStatus>, String> {
    let conn = db.conn.lock().unwrap();
    crate::history::health::get_url_health(&conn, url_id).map_err(|e| e.to_string())
}
