use serde::{Deserialize, Serialize};
use std::fs;
use tauri::State;

use crate::database::repository::{
    get_setting as repo_get_setting, set_setting as repo_set_setting,
};
use crate::database::DbState;
use crate::error::AppResult;

#[derive(Debug, Serialize, Deserialize)]
pub struct AppInfo {
    pub app_dir: String,
    pub db_path: String,
    pub db_size_bytes: u64,
    pub temp_dir: String,
    pub logs_dir: String,
}

#[tauri::command]
pub fn get_app_info(db: State<'_, DbState>) -> AppResult<AppInfo> {
    let db_path = db.app_dir.join("archive.db");
    let db_size_bytes = fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);

    Ok(AppInfo {
        app_dir: db.app_dir.to_string_lossy().to_string(),
        db_path: db_path.to_string_lossy().to_string(),
        db_size_bytes,
        temp_dir: db.temp_dir.to_string_lossy().to_string(),
        logs_dir: db.logs_dir.to_string_lossy().to_string(),
    })
}

#[tauri::command]
pub fn get_setting(key: String, db: State<'_, DbState>) -> AppResult<Option<String>> {
    if key == "ai_key" {
        let raw = crate::security::secret_store::get_secret("ai_key", &db.app_dir).unwrap_or(None);
        return Ok(raw.map(|k| {
            let trimmed = k.trim();
            if trimmed.len() > 8 {
                let prefix = &trimmed[..3];
                let suffix = &trimmed[trimmed.len() - 4..];
                format!("{}••••{}", prefix, suffix)
            } else if !trimmed.is_empty() {
                "••••••••".to_string()
            } else {
                String::new()
            }
        }));
    }
    let conn = db.conn.lock().unwrap();
    repo_get_setting(&conn, &key)
}

#[tauri::command]
pub fn set_setting(key: String, value: String, db: State<'_, DbState>) -> AppResult<()> {
    if key == "ai_key" {
        // Do not overwrite real stored key if user left masked value unchanged
        if value.contains('•') {
            return Ok(());
        }
        crate::security::secret_store::set_secret("ai_key", &value, &db.app_dir)
            .map_err(|e| crate::error::AppError::Other(format!("无法安全存储 API Key: {}", e)))?;
        let conn = db.conn.lock().unwrap();
        let _ = conn.execute("DELETE FROM settings WHERE key = 'ai_key'", []);
        return Ok(());
    }
    let conn = db.conn.lock().unwrap();
    repo_set_setting(&conn, &key, &value)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AiRequestPayload {
    pub prompt: String,
    pub system_prompt: Option<String>,
}

#[tauri::command]
pub async fn call_ai_completion(
    payload: AiRequestPayload,
    db: State<'_, DbState>,
) -> Result<String, String> {
    let (base_url, model) = {
        let conn = db.conn.lock().unwrap();
        let b_url = repo_get_setting(&conn, "ai_base_url")
            .ok()
            .flatten()
            .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
        let mdl = repo_get_setting(&conn, "ai_model")
            .ok()
            .flatten()
            .unwrap_or_else(|| "gpt-4o-mini".to_string());
        (b_url, mdl)
    };

    let api_key = crate::security::secret_store::get_secret("ai_key", &db.app_dir)
        .unwrap_or(None)
        .unwrap_or_default();

    let trimmed_base = base_url.trim().trim_end_matches('/');
    let is_local = trimmed_base.contains("localhost") || trimmed_base.contains("127.0.0.1");

    if api_key.trim().is_empty() && !is_local {
        return Err("未配置 API Key。若使用在线商用模型，请先在“系统设置”页面配置 API Key；若为本地 Ollama 则可免 Key。".to_string());
    }

    let endpoint = format!("{}/chat/completions", trimmed_base);

    let mut messages = Vec::new();
    if let Some(sys) = payload.system_prompt {
        messages.push(serde_json::json!({
            "role": "system",
            "content": sys,
        }));
    }
    messages.push(serde_json::json!({
        "role": "user",
        "content": payload.prompt,
    }));

    let request_body = serde_json::json!({
        "model": model,
        "messages": messages,
        "temperature": 0.5,
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;

    let mut req = client.post(&endpoint).json(&request_body);
    if !api_key.trim().is_empty() {
        req = req.header("Authorization", format!("Bearer {}", api_key.trim()));
    }

    let resp = req
        .send()
        .await
        .map_err(|e| format!("AI 服务请求失败: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let err_text = resp.text().await.unwrap_or_default();
        return Err(format!("AI 接口错误 (HTTP {}): {}", status, err_text));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("响应解析失败: {}", e))?;

    let answer = json
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|first| first.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or_else(|| "响应中未包含有效文本内容".to_string())?;

    Ok(answer.to_string())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TestAiPayload {
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TestAiResult {
    pub success: bool,
    pub latency_ms: u64,
    pub message: String,
}

#[tauri::command]
pub async fn test_ai_connection(
    payload: TestAiPayload,
    db: State<'_, DbState>,
) -> Result<TestAiResult, String> {
    let start_time = std::time::Instant::now();
    let trimmed_base = payload.base_url.trim().trim_end_matches('/');
    if trimmed_base.is_empty() {
        return Err("Base URL 不能为空".to_string());
    }

    let is_local = trimmed_base.contains("localhost") || trimmed_base.contains("127.0.0.1");
    let mut key = payload.api_key.unwrap_or_default();

    if (key.trim().is_empty() || key.contains('•')) && !is_local {
        if let Ok(Some(real_key)) = crate::security::secret_store::get_secret("ai_key", &db.app_dir)
        {
            key = real_key;
        }
    }

    if key.trim().is_empty() && !is_local {
        return Err(
            "云端服务商需配置 API Key；若使用本地 Ollama / LM Studio 可免填写。".to_string(),
        );
    }

    let endpoint = format!("{}/chat/completions", trimmed_base);
    let request_body = serde_json::json!({
        "model": payload.model.trim(),
        "messages": [
            { "role": "user", "content": "ping" }
        ],
        "max_tokens": 10,
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;

    let mut req = client.post(&endpoint).json(&request_body);
    if !key.trim().is_empty() {
        req = req.header("Authorization", format!("Bearer {}", key.trim()));
    }

    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            if is_local {
                return Err(format!(
                    "无法连接到本地服务 ({})。请确保本地模型服务（如 Ollama / LM Studio）已启动且端口无阻断。底层错误: {}",
                    trimmed_base, e
                ));
            } else {
                return Err(format!("网络请求失败，无法连接到端点: {}", e));
            }
        }
    };

    let status = resp.status();
    let latency_ms = start_time.elapsed().as_millis() as u64;

    if !status.is_success() {
        let err_text = resp.text().await.unwrap_or_default();
        if status.as_u16() == 401 {
            return Err("身份认证失败 (HTTP 401): API Key 无效或未提供。".to_string());
        } else if status.as_u16() == 404 {
            return Err(format!(
                "未找到接口或模型 (HTTP 404)。请检查端点 URL 或确认模型名称 '{}' 是否正确下载。服务端反馈: {}",
                payload.model, err_text
            ));
        } else {
            return Err(format!("服务返回错误 (HTTP {}): {}", status, err_text));
        }
    }

    // Attempt to verify json
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("响应格式非标准 JSON: {}", e))?;
    let has_content = json
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|f| f.get("message"))
        .is_some();

    if !has_content {
        return Ok(TestAiResult {
            success: true,
            latency_ms,
            message: format!(
                "服务通畅 (耗时 {}ms)，但返回未包含 choices 字段，请确认模型已就绪。",
                latency_ms
            ),
        });
    }

    Ok(TestAiResult {
        success: true,
        latency_ms,
        message: format!("连接成功！模型响应正常 (耗时 {}ms)", latency_ms),
    })
}
