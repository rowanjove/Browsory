use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use tauri::State;
use url::Url;

use crate::ai::http::{
    credential_ref_for_endpoint, read_response_body_limited, read_response_text_limited,
    MAX_AI_RESPONSE_BYTES,
};
use crate::database::repository::{
    get_setting as repo_get_setting, set_setting as repo_set_setting,
};
use crate::database::DbState;
use crate::error::AppResult;

const WEBDAV_CONFIG_KEY: &str = "webdav_sync_config";
const AI_CREDENTIAL_PREFIX: &str = "ai_credential_";
const AI_EMBEDDING_CREDENTIAL_PREFIX: &str = "ai_embedding_credential_";

fn is_credential_key(key: &str) -> bool {
    matches!(key, "ai_key" | "ai_embedding_key")
        || key.starts_with(AI_CREDENTIAL_PREFIX)
        || key.starts_with(AI_EMBEDDING_CREDENTIAL_PREFIX)
}

fn legacy_credential_key(key: &str) -> Option<&'static str> {
    if key.starts_with(AI_CREDENTIAL_PREFIX) {
        Some("ai_key")
    } else if key.starts_with(AI_EMBEDDING_CREDENTIAL_PREFIX) {
        Some("ai_embedding_key")
    } else {
        None
    }
}

fn mask_secret(value: &str) -> String {
    let chars: Vec<char> = value.trim().chars().collect();
    if chars.len() > 8 {
        let prefix: String = chars.iter().take(3).collect();
        let suffix: String = chars.iter().skip(chars.len() - 4).collect();
        format!("{}••••{}", prefix, suffix)
    } else if !chars.is_empty() {
        "••••••••".to_string()
    } else {
        String::new()
    }
}

pub(crate) fn webdav_password_secret_key(config: &crate::sync::webdav::WebDavConfig) -> String {
    let server = Url::parse(config.server_url.trim())
        .map(|mut url| {
            url.set_fragment(None);
            let path = url.path().trim_end_matches('/').to_string();
            if !path.is_empty() {
                url.set_path(&path);
            }
            url.to_string()
        })
        .unwrap_or_else(|_| config.server_url.trim().to_string());
    let scope = format!("{}\n{}", server, config.username.trim());
    format!(
        "webdav_password_{}",
        hex::encode(Sha256::digest(scope.as_bytes()))
    )
}

fn sanitize_webdav_config(value: &str, app_dir: &std::path::Path) -> AppResult<String> {
    let mut config: crate::sync::webdav::WebDavConfig = serde_json::from_str(value)
        .map_err(|e| crate::error::AppError::Parse(format!("WebDAV 配置格式无效: {}", e)))?;
    let password = config.password.trim().to_string();
    if !password.is_empty() {
        let key = webdav_password_secret_key(&config);
        crate::security::secret_store::set_secret(&key, &password, app_dir).map_err(|e| {
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
pub async fn get_app_info(db: State<'_, DbState>) -> AppResult<AppInfo> {
    let db = db.inner().clone();
    super::blocking::run(move || {
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
    })
    .await?
}

#[tauri::command]
pub fn quit_application(app: tauri::AppHandle) {
    app.exit(0);
}

fn validate_release_html_url(raw: &str) -> Option<String> {
    let parsed = url::Url::parse(raw).ok()?;
    if parsed.scheme() == "https"
        && parsed.host_str() == Some("github.com")
        && parsed.path().starts_with("/rowanjove/browsory/")
    {
        Some(parsed.to_string())
    } else {
        None
    }
}

#[tauri::command]
pub async fn check_for_updates() -> Result<UpdateCheckResult, String> {
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
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

    let body = read_response_body_limited(resp, 512 * 1024).await?;
    let json: serde_json::Value =
        serde_json::from_slice(&body).map_err(|e| format!("解析更新信息失败: {}", e))?;
    let tag = json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim_start_matches('v')
        .to_string();
    let html_url = json
        .get("html_url")
        .and_then(|v| v.as_str())
        .and_then(validate_release_html_url);
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
pub async fn get_setting(key: String, db: State<'_, DbState>) -> AppResult<Option<String>> {
    let operation = db.begin_operation()?;
    let db = db.inner().clone();
    super::blocking::run(move || {
        operation.check()?;

        let result = (|| {
            if is_credential_key(&key) {
                let raw =
                    crate::security::secret_store::get_secret(&key, &db.app_dir).map_err(|e| {
                        crate::error::AppError::Security(format!("读取凭据失败: {}", e))
                    })?;
                return Ok(raw.map(|secret| mask_secret(&secret)));
            }
            if key == WEBDAV_CONFIG_KEY {
                return operation.with_conn(|conn| {
                    let Some(raw) = repo_get_setting(conn, &key)? else {
                        return Ok(None);
                    };
                    let sanitized = sanitize_webdav_config(&raw, &db.app_dir)?;
                    if sanitized != raw {
                        // Read, secret migration and row cleanup share the same
                        // connection lock as set_setting so a stale read cannot
                        // overwrite a newer server/account configuration.
                        operation.check()?;
                        repo_set_setting(conn, &key, &sanitized)?;
                    }
                    Ok(Some(sanitized))
                });
            }
            operation.with_read_conn(|conn| repo_get_setting(conn, &key))
        })();
        operation.check()?;
        result
    })
    .await?
}

#[tauri::command]
pub async fn set_setting(key: String, value: String, db: State<'_, DbState>) -> AppResult<()> {
    let operation = db.begin_operation()?;
    let db = db.inner().clone();
    super::blocking::run(move || {
        operation.check()?;

        let result = (|| {
            if is_credential_key(&key) {
                // Do not overwrite real stored key if user left masked value unchanged
                if value.contains('•') {
                    // An explicit Save after upgrading may bind a legacy global
                    // secret to the currently selected endpoint. Never do this on
                    // reads or tests, and never overwrite an already bound value.
                    if let Some(legacy_key) = legacy_credential_key(&key) {
                        let current = crate::security::secret_store::get_secret(&key, &db.app_dir)
                            .map_err(|e| {
                                crate::error::AppError::Security(format!("读取凭据失败: {}", e))
                            })?;
                        if current.is_none() {
                            if let Some(legacy) =
                                crate::security::secret_store::get_secret(legacy_key, &db.app_dir)
                                    .map_err(|e| {
                                    crate::error::AppError::Security(format!(
                                        "读取旧凭据失败: {}",
                                        e
                                    ))
                                })?
                            {
                                crate::security::secret_store::set_secret(
                                    &key,
                                    &legacy,
                                    &db.app_dir,
                                )
                                .map_err(|e| {
                                    crate::error::AppError::Security(format!(
                                        "迁移旧凭据失败: {}",
                                        e
                                    ))
                                })?;
                            }
                        }
                    }
                    return Ok(());
                }
                crate::security::secret_store::set_secret(&key, &value, &db.app_dir).map_err(
                    |e| crate::error::AppError::Other(format!("无法安全存储 API Key: {}", e)),
                )?;
                let conn_arc = operation.connection()?;
                let conn = conn_arc.lock()?;
                let _ = conn.execute(
                    "DELETE FROM settings WHERE key = ?1",
                    rusqlite::params![key],
                );
                return Ok(());
            }
            if key == WEBDAV_CONFIG_KEY {
                let conn_arc = operation.connection()?;
                let conn = conn_arc.lock()?;
                let sanitized = sanitize_webdav_config(&value, &db.app_dir)?;
                return repo_set_setting(&conn, &key, &sanitized);
            }
            let conn_arc = operation.connection()?;
            let conn = conn_arc.lock()?;
            repo_set_setting(&conn, &key, &value)
        })();
        operation.check()?;
        result
    })
    .await?
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
    let operation = db.begin_operation().map_err(|e| e.to_string())?;
    operation.run(async {

    let app_dir = db.app_dir.clone();
    let (base_url, model, api_key) = super::blocking::with_operation(&operation, move |operation| {
        operation.with_conn(|conn| crate::commands::ai::get_ai_credentials(conn, &app_dir))
    }).await.map_err(|e| e.to_string())?;

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

    let client = crate::ai::http::ai_client(trimmed_base, std::time::Duration::from_secs(60))
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
        let err_text = read_response_text_limited(resp, MAX_AI_RESPONSE_BYTES)
            .await
            .unwrap_or_else(|e| e);
        return Err(format!("AI 接口错误 (HTTP {}): {}", status, err_text));
    }

    let body = read_response_body_limited(resp, MAX_AI_RESPONSE_BYTES)
        .await
        .map_err(|e| e.to_string())?;
    let json: serde_json::Value = serde_json::from_slice(&body)
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

    }).await.map_err(|e| e.to_string())?
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
    let operation = db.begin_operation().map_err(|e| e.to_string())?;
    operation.run(async {

    let start_time = std::time::Instant::now();
    let trimmed_base = payload.base_url.trim().trim_end_matches('/');
    if trimmed_base.is_empty() {
        return Err("Base URL 不能为空".to_string());
    }

    validate_ai_endpoint(trimmed_base)?;
    let is_local = crate::commands::settings::is_local_ai_endpoint(trimmed_base);
    let mut key = payload.api_key.unwrap_or_default();

    if key.trim().is_empty() || key.contains('•') {
        let app_dir = db.app_dir.clone();
        let credential_ref = credential_ref_for_endpoint(trimmed_base, false);
        key = super::blocking::with_operation(&operation, move |_| {
            crate::security::secret_store::get_secret(&credential_ref, &app_dir)
                .map_err(|e| crate::error::AppError::Security(format!("读取 AI 凭据失败: {e}")))
        }).await.map_err(|e| e.to_string())?.unwrap_or_default();
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

    let client = crate::ai::http::ai_client(trimmed_base, std::time::Duration::from_secs(12))
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
        let err_text = read_response_text_limited(resp, MAX_AI_RESPONSE_BYTES)
            .await
            .unwrap_or_else(|e| e);
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
    let body = read_response_body_limited(resp, MAX_AI_RESPONSE_BYTES)
        .await
        .map_err(|e| e.to_string())?;
    let json: serde_json::Value = serde_json::from_slice(&body)
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

    }).await.map_err(|e| e.to_string())?
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
    let operation = db.begin_operation().map_err(|e| e.to_string())?;
    operation
        .run(async {
            let start_time = std::time::Instant::now();
            let model = payload.model.trim();
            if model.is_empty() {
                return Err("向量模型名称不能为空".to_string());
            }

            let base_url = match payload.base_url {
                Some(b) if !b.trim().is_empty() => b.trim().to_string(),
                _ => super::blocking::with_operation(&operation, |operation| {
                    operation.with_conn(|conn| {
                        Ok(repo_get_setting(conn, "ai_embedding_base_url")?
                            .filter(|value| !value.trim().is_empty())
                            .unwrap_or_else(|| "https://api.openai.com/v1".into()))
                    })
                })
                .await
                .map_err(|e| e.to_string())?,
            };

            let mut key = match payload.api_key {
                Some(k) if !k.contains('•') && !k.trim().is_empty() => k.trim().to_string(),
                _ => String::new(),
            };

            let trimmed_base = base_url.trim().trim_end_matches('/');
            if trimmed_base.is_empty() {
                return Err("Base URL 不能为空，请配置向量服务 Base URL".to_string());
            }

            validate_ai_endpoint(trimmed_base)?;
            let is_local = crate::commands::settings::is_local_ai_endpoint(trimmed_base);
            if key.trim().is_empty() || key.contains('•') {
                let app_dir = db.app_dir.clone();
                let credential_ref = credential_ref_for_endpoint(trimmed_base, true);
                key = super::blocking::with_operation(&operation, move |_| {
                    crate::security::secret_store::get_secret(&credential_ref, &app_dir).map_err(
                        |e| {
                            crate::error::AppError::Security(format!(
                                "读取 Embedding 凭据失败: {e}"
                            ))
                        },
                    )
                })
                .await
                .map_err(|e| e.to_string())?
                .unwrap_or_default();
            }
            if key.trim().is_empty() && !is_local {
                return Err("在线服务商需提供有效 API Key；若为本地 Ollama 可免 Key。".to_string());
            }

            let sample_texts = ["Browsory embedding ping test"];
            let vectors =
                crate::ai::embedding::fetch_embeddings(trimmed_base, &key, model, &sample_texts)
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
        })
        .await
        .map_err(|e| e.to_string())?
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
    use super::{
        is_local_ai_endpoint, mask_secret, validate_ai_endpoint, webdav_password_secret_key,
        WEBDAV_CONFIG_KEY,
    };
    use crate::database::repository;
    use crate::database::DbState;
    use crate::security::vault::VaultManager;
    use std::sync::Arc;
    use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};

    fn ipc_request(command: &str, body: serde_json::Value) -> tauri::webview::InvokeRequest {
        tauri::webview::InvokeRequest {
            cmd: command.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "http://tauri.localhost".parse().unwrap(),
            body: tauri::ipc::InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.into(),
        }
    }

    fn fixture_state(root: &std::path::Path, vault: Arc<VaultManager>) -> DbState {
        DbState {
            _directory_lock: None,
            vault,
            app_dir: root.to_path_buf(),
            temp_dir: root.join("temp"),
            logs_dir: root.join("logs"),
        }
    }

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

    #[test]
    fn secret_mask_is_utf8_safe_and_does_not_reveal_short_values() {
        assert_eq!(mask_secret("short"), "••••••••");
        assert_eq!(mask_secret("😀😀😀😀😀😀😀😀😀"), "😀😀😀••••😀😀😀😀");
        assert_eq!(mask_secret("abcdefghi"), "abc••••fghi");
    }

    #[test]
    fn release_html_url_validation_allows_only_expected_github_repo_https() {
        use super::validate_release_html_url;
        assert_eq!(
            validate_release_html_url("https://github.com/rowanjove/browsory/releases/tag/v0.1.3")
                .as_deref(),
            Some("https://github.com/rowanjove/browsory/releases/tag/v0.1.3")
        );
        assert!(
            validate_release_html_url("http://github.com/rowanjove/browsory/releases").is_none()
        );
        assert!(validate_release_html_url("https://evil.example/releases").is_none());
        assert!(validate_release_html_url("javascript:alert(1)").is_none());
    }

    #[test]
    fn get_webdav_setting_persists_sanitized_password_free_config() {
        let root = std::env::temp_dir().join(format!(
            "browsory_webdav_setting_migration_{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let vault = Arc::new(VaultManager::unlocked_fixture(root.clone()));
        let raw = serde_json::json!({
            "enabled": true,
            "server_url": "https://dav.example.test/dav/",
            "username": "alice",
            "password": "migration•secret",
            "remote_dir": "/browsory"
        })
        .to_string();
        let key_config: crate::sync::webdav::WebDavConfig = serde_json::from_str(&raw).unwrap();
        let scoped_secret_key = webdav_password_secret_key(&key_config);
        vault
            .with_conn(|conn| repository::set_setting(conn, WEBDAV_CONFIG_KEY, &raw))
            .unwrap();
        vault
            .with_conn(|conn| {
                conn.execute_batch(
                    "CREATE TRIGGER test_webdav_migration_write_failure
                     BEFORE INSERT ON settings
                     WHEN NEW.key = 'webdav_sync_config'
                     BEGIN SELECT RAISE(ABORT, 'injected WebDAV migration write failure'); END;",
                )?;
                Ok(())
            })
            .unwrap();

        let state = fixture_state(&root, vault.clone());
        let app = mock_builder()
            .manage(state)
            .invoke_handler(tauri::generate_handler![super::get_setting])
            .build(mock_context(noop_assets()))
            .unwrap();
        let webview = tauri::WebviewWindowBuilder::new(
            &app,
            "webdav-setting-migration-ipc",
            Default::default(),
        )
        .build()
        .unwrap();

        assert!(get_ipc_response(
            &webview,
            ipc_request("get_setting", serde_json::json!({"key": WEBDAV_CONFIG_KEY}),),
        )
        .is_err());
        vault
            .with_conn(|conn| {
                assert_eq!(
                    repository::get_setting(conn, WEBDAV_CONFIG_KEY)?.as_deref(),
                    Some(raw.as_str())
                );
                conn.execute_batch("DROP TRIGGER test_webdav_migration_write_failure;")?;
                Ok(())
            })
            .unwrap();
        assert_eq!(
            crate::security::secret_store::get_secret(&scoped_secret_key, &root)
                .unwrap()
                .as_deref(),
            Some("migration•secret")
        );

        let returned = get_ipc_response(
            &webview,
            ipc_request("get_setting", serde_json::json!({"key": WEBDAV_CONFIG_KEY})),
        )
        .unwrap()
        .deserialize::<Option<String>>()
        .unwrap()
        .expect("existing WebDAV configuration must be returned");
        let parsed: crate::sync::webdav::WebDavConfig = serde_json::from_str(&returned).unwrap();
        assert!(parsed.password.is_empty());

        vault
            .with_conn(|conn| {
                let persisted = repository::get_setting(conn, WEBDAV_CONFIG_KEY)?.unwrap();
                let persisted_config: crate::sync::webdav::WebDavConfig =
                    serde_json::from_str(&persisted).unwrap();
                assert!(persisted_config.password.is_empty());
                Ok(())
            })
            .unwrap();
        assert_eq!(
            crate::security::secret_store::get_secret(&scoped_secret_key, &root)
                .unwrap()
                .as_deref(),
            Some("migration•secret")
        );

        drop(webview);
        drop(app);
        let _ = crate::security::secret_store::delete_secret(&scoped_secret_key, &root);
        vault.lock().unwrap();
        drop(vault);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn webdav_credentials_are_scoped_to_normalized_server_and_account() {
        let config = |server_url: &str, username: &str| crate::sync::webdav::WebDavConfig {
            enabled: false,
            server_url: server_url.to_string(),
            username: username.to_string(),
            password: String::new(),
            remote_dir: "/browsory".into(),
        };
        let original =
            webdav_password_secret_key(&config("https://DAV.example.test/dav/", "alice"));
        assert_eq!(
            original,
            webdav_password_secret_key(&config("https://dav.example.test/dav", "alice")),
            "host casing and a trailing path slash do not create a different endpoint"
        );
        assert_ne!(
            original,
            webdav_password_secret_key(&config("https://dav.example.test/dav", "bob"))
        );
        assert_ne!(
            original,
            webdav_password_secret_key(&config("https://other.example.test/dav", "alice"))
        );
    }
}
