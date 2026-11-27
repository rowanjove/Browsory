use rusqlite::Connection;
use tauri::State;

use crate::ai::embedding::{
    fetch_embeddings, validate_embedding_batch_size, validate_embedding_result_limit,
};
use crate::ai::http::credential_ref_for_endpoint;
use crate::database::models::{
    EmbeddingIndexingStatus, HybridSearchResult, InterestEvolution, SimilarPageItem, TopicItem,
};
use crate::database::repository::{
    find_similar_pages as repo_find_similar_pages,
    get_embedding_indexing_status as repo_get_embedding_indexing_status,
    get_interest_evolution_for_ai as repo_get_interest_evolution_for_ai,
    get_unindexed_urls as repo_get_unindexed_urls,
    hybrid_search_history as repo_hybrid_search_history,
    query_all_topics_readonly as repo_query_all_topics_readonly,
    query_interest_evolution_readonly as repo_query_interest_evolution_readonly,
    save_page_embeddings as repo_save_page_embeddings,
};
use crate::database::DbState;
use crate::error::{AppError, AppResult};
use crate::security::secret_store;

const DEFAULT_EMBEDDING_MODEL: &str = "text-embedding-3-small";

#[cfg(test)]
mod acceptance_tests {
    use super::*;
    use crate::ai::test_http::FixtureServer;
    use crate::security::vault::VaultManager;
    use std::sync::Arc;
    use std::time::Duration;
    use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};

    #[test]
    fn ai_commands_exclude_private_history_in_real_loopback_requests() {
        let embedding =
            FixtureServer::json(serde_json::json!({"data":[{"index":0,"embedding":[1.0,0.0]}]}));
        let chat = FixtureServer::json(
            serde_json::json!({"choices":[{"message":{"content":"Synthetic report"}}]}),
        );
        let root =
            std::env::temp_dir().join(format!("browsory_ai_policy_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let vault = Arc::new(VaultManager::unlocked_fixture(root.clone()));
        vault.with_conn(|conn| {
            conn.execute_batch("INSERT INTO sources(id,browser,profile,source_type,history_path) VALUES(1,'chrome','Default','sqlite','synthetic');
                INSERT INTO urls(id,url,title,domain) VALUES
                (1,'https://public.example/page','PUBLIC_PAGE','public.example'),
                (2,'https://private.example/page','PRIVATE_PAGE','private.example'),
                (3,'http://[::1]/page','INTERNAL_PAGE','[::1]'),
                (4,'file:///C:/synthetic/note.txt','FILE_PAGE',''),
                (5,'https://public.example/ÄRZTE','UNICODE_PRIVATE_PAGE','public.example');
                INSERT INTO topics(id,name,category,created_at) VALUES(1001,'PUBLIC_TOPIC','fixture',0),(1002,'PRIVATE_TOPIC','fixture',0),(1003,'INTERNAL_TOPIC','fixture',0),(1004,'FILE_TOPIC','fixture',0);
                INSERT INTO url_topics(url_id,topic_id) VALUES(1,1001),(2,1001),(2,1002),(3,1003),(4,1004);")?;
            let now = chrono::Utc::now().timestamp_millis();
            for id in 1..=5 {
                conn.execute("INSERT INTO visits(source_id,url_id,source_visit_id,visit_time,event_hash,imported_at) VALUES(1,?1,?1,?2,?3,0)",rusqlite::params![id,now,format!("ai-http-fixture-{id}")])?;
            }
            crate::database::repository::add_privacy_rule(conn,"private.example","private")?;
            crate::database::repository::add_privacy_rule(conn,"ärzte","private")?;
            for (key, value) in [("ai_embedding_base_url",embedding.base_url.as_str()),("ai_base_url",chat.base_url.as_str()),("ai_embedding_model","fixture-embedding"),("ai_model","fixture-chat")] {
                crate::database::repository::set_setting(conn,key,value)?;
            }
            Ok(())
        }).unwrap();
        let state = DbState {
            _directory_lock: None,
            vault: vault.clone(),
            app_dir: root.clone(),
            temp_dir: root.join("temp"),
            logs_dir: root.join("logs"),
        };
        let app = mock_builder()
            .manage(state)
            .invoke_handler(tauri::generate_handler![
                super::generate_embeddings_batch,
                super::generate_ai_period_comparison
            ])
            .build(mock_context(noop_assets()))
            .unwrap();
        let webview = tauri::WebviewWindowBuilder::new(&app, "ai-policy-test", Default::default())
            .build()
            .unwrap();
        for (command, body) in [
            (
                "generate_embeddings_batch",
                serde_json::json!({"model":null,"batchSize":30}),
            ),
            (
                "generate_ai_period_comparison",
                serde_json::json!({"days":30}),
            ),
        ] {
            let response = get_ipc_response(
                &webview,
                tauri::webview::InvokeRequest {
                    cmd: command.into(),
                    callback: tauri::ipc::CallbackFn(0),
                    error: tauri::ipc::CallbackFn(1),
                    url: "http://tauri.localhost".parse().unwrap(),
                    body: tauri::ipc::InvokeBody::Json(body),
                    headers: Default::default(),
                    invoke_key: INVOKE_KEY.into(),
                },
            );
            assert!(response.is_ok(), "{command}: {response:?}");
        }
        let embedding_request: serde_json::Value =
            serde_json::from_slice(&embedding.body.recv_timeout(Duration::from_secs(2)).unwrap())
                .unwrap();
        let inputs = embedding_request["input"].as_array().unwrap();
        assert_eq!(inputs.len(), 1);
        assert!(inputs[0].as_str().unwrap().contains("PUBLIC_PAGE"));
        let chat_request: serde_json::Value =
            serde_json::from_slice(&chat.body.recv_timeout(Duration::from_secs(2)).unwrap())
                .unwrap();
        let prompt = chat_request["messages"].to_string();
        assert!(prompt.contains("PUBLIC_TOPIC"));
        assert!(prompt.contains("本期首次访问 1 次"));
        for private in [
            "PRIVATE_TOPIC",
            "INTERNAL_TOPIC",
            "FILE_TOPIC",
            "PRIVATE_PAGE",
            "UNICODE_PRIVATE_PAGE",
            "INTERNAL_PAGE",
            "FILE_PAGE",
        ] {
            assert!(!prompt.contains(private), "private context sent: {private}");
            assert!(!embedding_request.to_string().contains(private));
        }
        vault
            .with_conn(|conn| {
                let indexed: i64 = conn.query_row(
                    "SELECT count(*) FROM page_embeddings WHERE url_id=1",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(indexed, 1);
                Ok(())
            })
            .unwrap();
        vault.lock().unwrap();
        drop(webview);
        drop(app);
        drop(vault);
        std::fs::remove_dir_all(root).unwrap();
    }
}

pub fn get_ai_credentials(
    conn: &Connection,
    app_dir: &std::path::Path,
) -> AppResult<(String, String, String)> {
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

    let credential_ref = credential_ref_for_endpoint(&base_url, false);
    let api_key = secret_store::get_secret(&credential_ref, app_dir)
        .map_err(|error| AppError::Security(format!("读取 AI 凭据失败: {}", error)))?
        .unwrap_or_default();
    Ok((base_url, model, api_key))
}

pub fn get_embedding_credentials(
    conn: &Connection,
    app_dir: &std::path::Path,
) -> AppResult<(String, String, String)> {
    let emb_base_url: String = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'ai_embedding_base_url'",
            [],
            |r| r.get(0),
        )
        .ok()
        .filter(|s: &String| !s.trim().is_empty())
        .unwrap_or_else(|| "https://api.openai.com/v1".to_string());

    let emb_model: String = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'ai_embedding_model'",
            [],
            |r| r.get(0),
        )
        .ok()
        .filter(|s: &String| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_EMBEDDING_MODEL.to_string());

    let credential_ref = credential_ref_for_endpoint(&emb_base_url, true);
    let emb_key = secret_store::get_secret(&credential_ref, app_dir)
        .map_err(|error| AppError::Security(format!("读取 Embedding 凭据失败: {}", error)))?
        .unwrap_or_default();

    Ok((emb_base_url, emb_model, emb_key))
}

#[tauri::command]
pub async fn get_embedding_status(
    model: Option<String>,
    db: State<'_, DbState>,
) -> Result<EmbeddingIndexingStatus, String> {
    let operation = db.begin_operation().map_err(|e| e.to_string())?;
    let db = db.inner().clone();
    super::blocking::with_tracked(&operation, move |operation| {
        let conn_arc = operation.read_connection().map_err(|e| e.to_string())?;
        let conn = conn_arc.lock().map_err(|e| e.to_string())?;
        let m = match model {
            Some(m) if !m.trim().is_empty() => m,
            _ => {
                get_embedding_credentials(&conn, &db.app_dir)
                    .map_err(|e| e.to_string())?
                    .1
            }
        };
        repo_get_embedding_indexing_status(&conn, &m).map_err(|e: AppError| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn generate_embeddings_batch(
    model: Option<String>,
    batch_size: Option<usize>,
    db: State<'_, DbState>,
) -> Result<usize, String> {
    let operation = db.begin_operation().map_err(|e| e.to_string())?;
    operation
        .run(async {
            let limit = validate_embedding_batch_size(batch_size).map_err(|e| e.to_string())?;

            let app_dir = db.app_dir.clone();
            let (base_url, configured_model, api_key) =
                super::blocking::with_operation(&operation, move |operation| {
                    operation.with_conn(|conn| get_embedding_credentials(conn, &app_dir))
                })
                .await
                .map_err(|e| e.to_string())?;
            crate::commands::settings::validate_ai_endpoint(&base_url)?;
            let emb_model = model.unwrap_or(configured_model);

            let candidate_model = emb_model.clone();
            let unindexed = super::blocking::with_operation(&operation, move |operation| {
                operation.with_conn(|conn| repo_get_unindexed_urls(conn, &candidate_model, limit))
            })
            .await
            .map_err(|e| e.to_string())?;

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

            let is_local = crate::commands::settings::is_local_ai_endpoint(&base_url);
            let request_texts: Vec<String> = texts
                .iter()
                .map(|text| crate::ai::privacy::sanitize_prompt_for_ai(text, is_local))
                .collect();
            let text_refs: Vec<&str> = request_texts.iter().map(|s| s.as_str()).collect();

            let embeddings = fetch_embeddings(&base_url, &api_key, &emb_model, &text_refs)
                .await
                .map_err(|e| e.to_string())?;

            let count = super::blocking::with_operation(&operation, move |operation| {
                let records: Vec<(i64, String, Vec<f32>, &str)> = unindexed
                    .into_iter()
                    .zip(texts)
                    .zip(embeddings)
                    .map(|(((url_id, _, _, _), sample), emb)| {
                        (url_id, sample, emb, emb_model.as_str())
                    })
                    .collect();
                operation.with_conn(|conn| repo_save_page_embeddings(conn, &records))
            })
            .await
            .map_err(|e| e.to_string())?;

            Ok(count)
        })
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn hybrid_search(
    query: String,
    limit: Option<usize>,
    db: State<'_, DbState>,
) -> Result<Vec<HybridSearchResult>, String> {
    let operation = db.begin_operation().map_err(|e| e.to_string())?;
    operation
        .run(async {
            let lim = validate_embedding_result_limit(limit, 30).map_err(|e| e.to_string())?;
            let trimmed = query.trim();
            if trimmed.is_empty() {
                return Ok(Vec::new());
            }

            let app_dir = db.app_dir.clone();
            let (base_url, emb_model, api_key) =
                super::blocking::with_operation(&operation, move |operation| {
                    operation.with_read_conn(|conn| get_embedding_credentials(conn, &app_dir))
                })
                .await
                .map_err(|e| e.to_string())?;
            crate::commands::settings::validate_ai_endpoint(&base_url)?;

            // Attempt to generate query vector if credentials exist
            let is_local = crate::commands::settings::is_local_ai_endpoint(&base_url);
            let query_vector = if !base_url.is_empty() && (!api_key.is_empty() || is_local) {
                let safe_query = crate::ai::privacy::sanitize_prompt_for_ai(trimmed, is_local);
                fetch_embeddings(&base_url, &api_key, &emb_model, &[safe_query.as_str()])
                    .await
                    .ok()
                    .and_then(|mut v| v.pop())
            } else {
                None
            };

            let query_text = trimmed.to_string();
            super::blocking::with_operation(&operation, move |operation| {
                operation.with_read_conn(|conn| {
                    repo_hybrid_search_history(
                        conn,
                        &query_text,
                        query_vector.as_deref(),
                        &emb_model,
                        lim,
                    )
                })
            })
            .await
            .map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_similar_pages(
    url_id: i64,
    model: Option<String>,
    limit: Option<usize>,
    db: State<'_, DbState>,
) -> Result<Vec<SimilarPageItem>, String> {
    let operation = db.begin_operation().map_err(|e| e.to_string())?;
    let db = db.inner().clone();
    super::blocking::with_tracked(&operation, move |operation| {
        let conn_arc = operation.read_connection().map_err(|e| e.to_string())?;
        let conn = conn_arc.lock().map_err(|e| e.to_string())?;
        let m = match model {
            Some(m) if !m.trim().is_empty() => m,
            _ => {
                get_embedding_credentials(&conn, &db.app_dir)
                    .map_err(|e| e.to_string())?
                    .1
            }
        };
        let lim = validate_embedding_result_limit(limit, 10).map_err(|e| e.to_string())?;
        repo_find_similar_pages(&conn, url_id, &m, lim).map_err(|e: AppError| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_interest_evolution(
    days: Option<u32>,
    db: State<'_, DbState>,
) -> Result<InterestEvolution, String> {
    let operation = db.begin_operation().map_err(|e| e.to_string())?;
    super::blocking::with_tracked(&operation, move |operation| {
        let topic_count: i64 = operation
            .with_read_conn(|conn| {
                conn.query_row("SELECT count(*) FROM topics", [], |r| r.get(0))
                    .map_err(Into::into)
            })
            .map_err(|e| e.to_string())?;
        if topic_count < crate::ai::topics::DEFAULT_TOPICS.len() as i64 {
            operation
                .with_conn(crate::ai::topics::ensure_default_topics)
                .map_err(|e| e.to_string())?;
        }
        {
            let conn_arc = operation.connection().map_err(|e| e.to_string())?;
            if let Some(conn) = conn_arc.try_lock().map_err(|e| e.to_string())? {
                crate::ai::topics::auto_tag_unclassified_urls(&conn, 300)
                    .map_err(|e: AppError| e.to_string())?;
            };
        }
        operation
            .with_read_conn(|conn| repo_query_interest_evolution_readonly(conn, days.unwrap_or(30)))
            .map_err(|e: AppError| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_all_topics(db: State<'_, DbState>) -> Result<Vec<TopicItem>, String> {
    let operation = db.begin_operation().map_err(|e| e.to_string())?;
    super::blocking::with_tracked(&operation, move |operation| {
        let topic_count: i64 = operation
            .with_read_conn(|conn| {
                conn.query_row("SELECT count(*) FROM topics", [], |r| r.get(0))
                    .map_err(Into::into)
            })
            .map_err(|e| e.to_string())?;
        if topic_count < crate::ai::topics::DEFAULT_TOPICS.len() as i64 {
            operation
                .with_conn(crate::ai::topics::ensure_default_topics)
                .map_err(|e| e.to_string())?;
        }
        operation
            .with_read_conn(repo_query_all_topics_readonly)
            .map_err(|e: AppError| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn generate_ai_period_comparison(
    days: Option<u32>,
    db: State<'_, DbState>,
) -> Result<String, String> {
    let operation = db.begin_operation().map_err(|e| e.to_string())?;
    operation
        .run(async {
            let selected_days = days.unwrap_or(30);
            let evolution = super::blocking::with_operation(&operation, move |operation| {
                operation.with_conn(|conn| repo_get_interest_evolution_for_ai(conn, selected_days))
            })
            .await
            .map_err(|e| e.to_string())?;

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
        })
        .await
        .map_err(|e| e.to_string())?
}
