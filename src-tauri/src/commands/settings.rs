use serde::{Deserialize, Serialize};
use std::fs;
use tauri::State;

use crate::database::repository::{
    get_setting as repo_get_setting, set_setting as repo_set_setting,
};
use crate::database::DbState;
use crate::error::AppResult;

const WEBDAV_CONFIG_KEY: &str = "webdav_sync_config";
const WEBDAV_PASSWORD_SECRET: &str = "webdav_password";

fn sanitize_webdav_config(value: &str, app_dir: &std::path::Path) -> AppResult<String> {
    let mut config: crate::sync::webdav::WebDavConfig = serde_json::from_str(value)
        .map_err(|e| crate::error::AppError::Parse(format!("WebDAV 配置格式无效: {}", e)))?;
    let password = config.password.trim().to_string();
    if !password.is_empty() && !password.contains('•') {
        crate::security::secret_store::set_secret(WEBDAV_PASSWORD_SECRET, &password, app_dir)
            .map_err(|e| {
                crate::error::AppError::Security(format!("保存 WebDAV 密码失败: {}", e))
            })?;
    }
    config.password.clear();
    serde_json::to_string(&config).map_err(Into::into)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AppInfo {
    pub version: String,
    pub is_portable: bool,
    pub app_dir: String,
    pub db_path: String,
    pub db_size_bytes: u64,
    pub temp_dir: String,
    pub logs_dir: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub release_url: Option<String>,
    pub notes: Option<String>,
    pub update_available: bool,
}

#[tauri::command]
pub fn get_app_info(db: State<'_, DbState>) -> AppResult<AppInfo> {
    let db_path = db.app_dir.join("archive.db");
    let db_size_bytes = fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);
    let is_portable = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .map(|d| crate::database::connection::is_portable_install(&d))
        .unwrap_or(false);

    Ok(AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        is_portable,
        app_dir: db.app_dir.to_string_lossy().to_string(),
        db_path: db_path.to_string_lossy().to_string(),
        db_size_bytes,
        temp_dir: db.temp_dir.to_string_lossy().to_string(),
        logs_dir: db.logs_dir.to_string_lossy().to_string(),
    })
}

#[tauri::command]
pub fn quit_application(app: tauri::AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub async fn check_for_updates() -> Result<UpdateCheckResult, String> {
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .user_agent(format!("Browsory/{}", current_version))
        .build()
        .map_err(|e| format!("无法创建更新检查客户端: {}", e))?;

    let resp = client
        .get("https://api.github.com/repos/rowanjove/browsory/releases/latest")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("无法连接 GitHub 检查更新: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("检查更新失败: HTTP {}", resp.status()));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析更新信息失败: {}", e))?;
    let tag = json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim_start_matches('v')
        .to_string();
    let html_url = json
        .get("html_url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let notes = json
        .get("body")
        .and_then(|v| v.as_str())
        .map(|s| s.chars().take(1200).collect::<String>());

    let update_available = !tag.is_empty() && tag != current_version;
    Ok(UpdateCheckResult {
        current_version,
        latest_version: if tag.is_empty() { None } else { Some(tag) },
        release_url: html_url,
        notes,
        update_available,
    })
}

#[tauri::command]
pub fn get_setting(key: String, db: State<'_, DbState>) -> AppResult<Option<String>> {
    if key == "ai_key" || key == "ai_embedding_key" {
        let raw = crate::security::secret_store::get_secret(&key, &db.app_dir).unwrap_or(None);
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
    let value = repo_get_setting(&conn, &key)?;
    drop(conn);
    if key == WEBDAV_CONFIG_KEY {
        return value
            .map(|raw| sanitize_webdav_config(&raw, &db.app_dir))
            .transpose();
    }
    Ok(value)
}

#[tauri::command]
pub fn set_setting(key: String, value: String, db: State<'_, DbState>) -> AppResult<()> {
    if key == "ai_key" || key == "ai_embedding_key" {
        // Do not overwrite real stored key if user left masked value unchanged
        if value.contains('•') {
            return Ok(());
        }
        crate::security::secret_store::set_secret(&key, &value, &db.app_dir)
            .map_err(|e| crate::error::AppError::Other(format!("无法安全存储 API Key: {}", e)))?;
        let conn = db.conn.lock().unwrap();
        let _ = conn.execute(
            "DELETE FROM settings WHERE key = ?1",
            rusqlite::params![key],
        );
        return Ok(());
    }
    if key == WEBDAV_CONFIG_KEY {
        let sanitized = sanitize_webdav_config(&value, &db.app_dir)?;
        let conn = db.conn.lock().unwrap();
        return repo_set_setting(&conn, &key, &sanitized);
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
    validate_ai_endpoint(trimmed_base)?;
    let is_local = crate::commands::settings::is_local_ai_endpoint(trimmed_base);

    if api_key.trim().is_empty() && !is_local {
        return Err("未配置 API Key。若使用在线商用模型，请先在“系统设置”页面配置 API Key；若为本地 Ollama 则可免 Key。".to_string());
    }

    let endpoint = format!("{}/chat/completions", trimmed_base);

    let safe_prompt = crate::ai::privacy::sanitize_prompt_for_ai(&payload.prompt, is_local);

    let mut messages = Vec::new();
    if let Some(sys) = payload.system_prompt {
        messages.push(serde_json::json!({
            "role": "system",
            "content": crate::ai::privacy::sanitize_prompt_for_ai(&sys, is_local),
        }));
    }
    messages.push(serde_json::json!({
        "role": "user",
        "content": safe_prompt,
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

    validate_ai_endpoint(trimmed_base)?;
    let is_local = crate::commands::settings::is_local_ai_endpoint(trimmed_base);
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

#[derive(Debug, Serialize, Deserialize)]
pub struct TestEmbeddingPayload {
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub model: String,
}

#[tauri::command]
pub async fn test_embedding_connection(
    payload: TestEmbeddingPayload,
    db: State<'_, DbState>,
) -> Result<TestAiResult, String> {
    let start_time = std::time::Instant::now();
    let model = payload.model.trim();
    if model.is_empty() {
        return Err("向量模型名称不能为空".to_string());
    }

    let (conn_base_url, _, conn_api_key) = {
        let conn = db.conn.lock().unwrap();
        crate::commands::ai::get_embedding_credentials(&conn, &db.app_dir)
    };

    let base_url = match payload.base_url {
        Some(b) if !b.trim().is_empty() => b.trim().to_string(),
        _ => conn_base_url,
    };

    let mut key = match payload.api_key {
        Some(k) if !k.contains('•') && !k.trim().is_empty() => k.trim().to_string(),
        _ => conn_api_key,
    };

    if key.trim().is_empty() || key.contains('•') {
        if let Ok(Some(real_key)) =
            crate::security::secret_store::get_secret("ai_embedding_key", &db.app_dir)
        {
            key = real_key;
        }
    }

    let trimmed_base = base_url.trim().trim_end_matches('/');
    if trimmed_base.is_empty() {
        return Err("Base URL 不能为空，请配置向量服务 Base URL".to_string());
    }

    validate_ai_endpoint(trimmed_base)?;
    let is_local = crate::commands::settings::is_local_ai_endpoint(trimmed_base);
    if key.trim().is_empty() && !is_local {
        return Err("在线服务商需提供有效 API Key；若为本地 Ollama 可免 Key。".to_string());
    }

    let sample_texts = ["Browsory embedding ping test"];
    let vectors = crate::ai::embedding::fetch_embeddings(trimmed_base, &key, model, &sample_texts)
        .await
        .map_err(|e| format!("向量接口测试失败: {}", e))?;

    let dim = vectors.first().map(|v| v.len()).unwrap_or(0);
    let latency_ms = start_time.elapsed().as_millis() as u64;

    Ok(TestAiResult {
        success: true,
        latency_ms,
        message: format!(
            "向量端点连接成功！模型 '{}' 返回特征维度: {} 维 (耗时 {}ms)",
            model, dim, latency_ms
        ),
    })
}

/// Determines whether an AI endpoint is a local loopback service. Host parsing
/// is required so names such as `localhost.evil.example` are never trusted.
pub fn is_local_ai_endpoint(raw: &str) -> bool {
    let Ok(url) = url::Url::parse(raw.trim()) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    host.eq_ignore_ascii_case("localhost")
        || matches!(
            url.host(),
            Some(url::Host::Ipv4(ip)) if ip.is_loopback()
        )
        || matches!(
            url.host(),
            Some(url::Host::Ipv6(ip)) if ip.is_loopback()
        )
}

pub(crate) fn validate_ai_endpoint(raw: &str) -> Result<(), String> {
    let parsed = url::Url::parse(raw).map_err(|e| format!("AI 端点地址无效: {}", e))?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("AI 端点地址不得内嵌用户名或密码".into());
    }
    let local = is_local_ai_endpoint(raw);
    if parsed.scheme() != "https" && !(parsed.scheme() == "http" && local) {
        return Err("远程 AI 端点必须使用 HTTPS；HTTP 仅允许回环地址".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{is_local_ai_endpoint, validate_ai_endpoint};

    #[test]
    fn local_endpoint_detection_requires_loopback_host() {
        assert!(is_local_ai_endpoint("http://localhost:11434/v1"));
        assert!(is_local_ai_endpoint("http://127.0.0.1:11434/v1"));
        assert!(is_local_ai_endpoint("http://[::1]:11434/v1"));
        assert!(!is_local_ai_endpoint("http://ollama.localhost:11434/v1"));
        assert!(!is_local_ai_endpoint("https://localhost.evil.example/v1"));
        assert!(!is_local_ai_endpoint("https://evil.example/localhost/v1"));
    }

    #[test]
    fn remote_ai_endpoint_requires_https() {
        assert!(validate_ai_endpoint("https://api.example/v1").is_ok());
        assert!(validate_ai_endpoint("http://api.example/v1").is_err());
    }
}
