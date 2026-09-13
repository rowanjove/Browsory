use rusqlite::Connection;
use tauri::State;

use crate::ai::embedding::fetch_embeddings;
use crate::database::models::{
    EmbeddingIndexingStatus, HybridSearchResult, InterestEvolution, SimilarPageItem, TopicItem,
};
use crate::database::repository::{
    find_similar_pages as repo_find_similar_pages, get_all_topics as repo_get_all_topics,
    get_embedding_indexing_status as repo_get_embedding_indexing_status,
    get_interest_evolution as repo_get_interest_evolution,
    get_unindexed_urls as repo_get_unindexed_urls,
    hybrid_search_history as repo_hybrid_search_history,
    save_page_embeddings as repo_save_page_embeddings,
};
use crate::database::DbState;
use crate::error::AppError;
use crate::security::secret_store;

const DEFAULT_EMBEDDING_MODEL: &str = "text-embedding-3-small";

fn get_ai_credentials(conn: &Connection, app_dir: &std::path::Path) -> (String, String, String) {
    let base_url: String = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'ai_base_url'",
            [],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());

    let model: String = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'ai_model'",
            [],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| "gpt-4o-mini".to_string());

    let api_key = secret_store::get_secret("ai_key", app_dir)
        .unwrap_or(None)
        .unwrap_or_default();
    (base_url, model, api_key)
}

#[tauri::command]
pub async fn get_embedding_status(
    model: Option<String>,
    db: State<'_, DbState>,
) -> Result<EmbeddingIndexingStatus, String> {
    let conn = db.conn.lock().unwrap();
    let m = model.unwrap_or_else(|| DEFAULT_EMBEDDING_MODEL.to_string());
    repo_get_embedding_indexing_status(&conn, &m).map_err(|e: AppError| e.to_string())
}

#[tauri::command]
pub async fn generate_embeddings_batch(
    model: Option<String>,
    batch_size: Option<usize>,
    db: State<'_, DbState>,
) -> Result<usize, String> {
    let emb_model = model.unwrap_or_else(|| DEFAULT_EMBEDDING_MODEL.to_string());
    let limit = batch_size.unwrap_or(20);

    let (base_url, _chat_model, api_key) = {
        let conn = db.conn.lock().unwrap();
        get_ai_credentials(&conn, &db.app_dir)
    };

    let unindexed = {
        let conn = db.conn.lock().unwrap();
        repo_get_unindexed_urls(&conn, &emb_model, limit).map_err(|e| e.to_string())?
    };

    if unindexed.is_empty() {
        return Ok(0);
    }

    let texts: Vec<String> = unindexed
        .iter()
        .map(|(_, url, title, domain)| {
            if title.is_empty() {
                format!("{} {}", domain, url)
            } else {
                format!("{} {} {}", title, domain, url)
            }
        })
        .collect();

    let text_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();

    let embeddings = fetch_embeddings(&base_url, &api_key, &emb_model, &text_refs)
        .await
        .map_err(|e| e.to_string())?;

    let records: Vec<(i64, String, Vec<f32>, &str)> = unindexed
        .into_iter()
        .zip(texts.into_iter())
        .zip(embeddings.into_iter())
        .map(|(((url_id, _, _, _), sample), emb)| (url_id, sample, emb, emb_model.as_str()))
        .collect();

    let count = {
        let conn = db.conn.lock().unwrap();
        repo_save_page_embeddings(&conn, &records).map_err(|e| e.to_string())?
    };

    Ok(count)
}

#[tauri::command]
pub async fn hybrid_search(
    query: String,
    limit: Option<usize>,
    db: State<'_, DbState>,
) -> Result<Vec<HybridSearchResult>, String> {
    let lim = limit.unwrap_or(30);
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let (base_url, _, api_key) = {
        let conn = db.conn.lock().unwrap();
        get_ai_credentials(&conn, &db.app_dir)
    };

    // Attempt to generate query vector if credentials exist
    let query_vector =
        if !base_url.is_empty() && (!api_key.is_empty() || base_url.contains("localhost")) {
            fetch_embeddings(&base_url, &api_key, DEFAULT_EMBEDDING_MODEL, &[trimmed])
                .await
                .ok()
                .and_then(|mut v| v.pop())
        } else {
            None
        };

    let conn = db.conn.lock().unwrap();
    repo_hybrid_search_history(
        &conn,
        trimmed,
        query_vector.as_deref(),
        DEFAULT_EMBEDDING_MODEL,
        lim,
    )
    .map_err(|e: AppError| e.to_string())
}

#[tauri::command]
pub async fn get_similar_pages(
    url_id: i64,
    model: Option<String>,
    limit: Option<usize>,
    db: State<'_, DbState>,
) -> Result<Vec<SimilarPageItem>, String> {
    let conn = db.conn.lock().unwrap();
    let m = model.unwrap_or_else(|| DEFAULT_EMBEDDING_MODEL.to_string());
    repo_find_similar_pages(&conn, url_id, &m, limit.unwrap_or(10))
        .map_err(|e: AppError| e.to_string())
}

#[tauri::command]
pub async fn get_interest_evolution(
    days: Option<u32>,
    db: State<'_, DbState>,
) -> Result<InterestEvolution, String> {
    let conn = db.conn.lock().unwrap();
    repo_get_interest_evolution(&conn, days.unwrap_or(30)).map_err(|e: AppError| e.to_string())
}

#[tauri::command]
pub async fn get_all_topics(db: State<'_, DbState>) -> Result<Vec<TopicItem>, String> {
    let conn = db.conn.lock().unwrap();
    repo_get_all_topics(&conn).map_err(|e: AppError| e.to_string())
}

#[tauri::command]
pub async fn generate_ai_period_comparison(
    days: Option<u32>,
    db: State<'_, DbState>,
) -> Result<String, String> {
    let selected_days = days.unwrap_or(30);
    let evolution = {
        let conn = db.conn.lock().unwrap();
        repo_get_interest_evolution(&conn, selected_days).map_err(|e: AppError| e.to_string())?
    };

    let mut context = format!(
        "【统计区间】：最近 {} 天 vs 上一周期 {} 天\n\n",
        selected_days, selected_days
    );

    context.push_str("【快速上升的兴趣领域】：\n");
    if evolution.rising_topics.is_empty() {
        context.push_str("无明显激增领域\n");
    } else {
        for t in &evolution.rising_topics {
            context.push_str(&format!(
                "- {} ({})：本期 {} 次 / 上期 {} 次 (增长 +{}%)\n",
                t.name, t.category, t.current_visits, t.previous_visits, t.growth_rate
            ));
        }
    }

    context.push_str("\n【新涉猎的探索领域】：\n");
    if evolution.new_topics.is_empty() {
        context.push_str("暂无全新领域\n");
    } else {
        for t in &evolution.new_topics {
            context.push_str(&format!(
                "- {} ({})：本期首次访问 {} 次\n",
                t.name, t.category, t.current_visits
            ));
        }
    }

    context.push_str("\n【关注度淡化的领域】：\n");
    if evolution.declining_topics.is_empty() {
        context.push_str("无明显下降领域\n");
    } else {
        for t in &evolution.declining_topics {
            context.push_str(&format!(
                "- {} ({})：本期 {} 次 / 上期 {} 次 (变化 {}%)\n",
                t.name, t.category, t.current_visits, t.previous_visits, t.growth_rate
            ));
        }
    }

    let system_prompt = "你是一个敏锐的个人数字认知与行为分析专家。\n\
        用户提供其最近两个周期的知识领域与主题浏览演化数据。\n\
        请给出结构化、富有洞察力的【长期兴趣演化与认知焦点对比分析】：\n\
        1. 核心转移：指出用户的探索焦点从哪里转向了哪里；\n\
        2. 结构变化原因推测与价值沉淀；\n\
        3. 对下一步持续学习或研究建议。\n\
        语言保持专业、启发性、精炼。";

    let prompt = format!(
        "以下是用户近两个周期的真实主题浏览数据：\n{}\n请输出对比洞察报告：",
        context
    );

    crate::commands::settings::call_ai_completion(
        crate::commands::settings::AiRequestPayload {
            prompt,
            system_prompt: Some(system_prompt.to_string()),
        },
        db,
    )
    .await
}
