use reqwest::{Client, Method, StatusCode};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use url::Url;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebDavConfig {
    pub enabled: bool,
    pub server_url: String, // 例如 https://dav.jianguoyun.com/dav/
    pub username: String,
    pub password: String, // 应用授权密码 (存储在本地安全存储或加密设置中)
    #[serde(default = "default_remote_dir")]
    pub remote_dir: String, // 默认 /browsory
}

fn default_remote_dir() -> String {
    "/browsory".to_string()
}

pub struct WebDavClient {
    client: Client,
    config: WebDavConfig,
}

impl WebDavClient {
    pub fn new(config: WebDavConfig) -> AppResult<Self> {
        validate_server_url(&config.server_url)?;
        if config.username.trim().is_empty() {
            return Err(AppError::Other("WebDAV 用户名不能为空".into()));
        }
        if config.password.trim().is_empty() {
            return Err(AppError::Other("WebDAV 密码或应用令牌未配置".into()));
        }
        if config
            .remote_dir
            .split(['/', '\\'])
            .any(|part| part == "..")
        {
            return Err(AppError::Parse("WebDAV 远端目录不得包含 .. 路径段".into()));
        }
        let client = Client::builder()
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| AppError::Other(format!("构建 WebDAV HTTP 客户端失败: {}", e)))?;

        Ok(Self { client, config })
    }

    fn build_full_url(&self, relative_path: &str) -> String {
        let base = self.config.server_url.trim_end_matches('/');
        let rel = relative_path.trim_start_matches('/');
        format!("{}/{}", base, rel)
    }

    /// 测试 WebDAV 连接与鉴权
    pub async fn test_connection(&self) -> AppResult<()> {
        let url = self.build_full_url("");

        let req = self
            .client
            .request(Method::from_bytes(b"PROPFIND").unwrap(), &url)
            .basic_auth(&self.config.username, Some(&self.config.password))
            .header("Depth", "0")
            .send()
            .await;

        match req {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success()
                    || status == StatusCode::MULTI_STATUS
                    || status == StatusCode::NOT_FOUND
                {
                    Ok(())
                } else if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
                    Err(AppError::Other(
                        "WebDAV 认证失败，请检查用户名与密码或应用令牌".into(),
                    ))
                } else {
                    Err(AppError::Other(format!(
                        "WebDAV 服务器响应异常: HTTP {}",
                        status
                    )))
                }
            }
            Err(err) => Err(AppError::Other(format!(
                "无法连接到 WebDAV 服务器: {}",
                err
            ))),
        }
    }

    /// 确保远端文件夹存在 (MKCOL)
    pub async fn ensure_directory(&self, rel_dir: &str) -> AppResult<()> {
        let url = self.build_full_url(rel_dir);
        let resp = self
            .client
            .request(Method::from_bytes(b"MKCOL").unwrap(), &url)
            .basic_auth(&self.config.username, Some(&self.config.password))
            .send()
            .await
            .map_err(|e| AppError::Other(format!("MKCOL 创建目录失败: {}", e)))?;

        let status = resp.status();
        // 201 Created, 405 Method Not Allowed (已存在), 200 OK 均视为成功
        if status == StatusCode::CREATED
            || status == StatusCode::METHOD_NOT_ALLOWED
            || status.is_success()
        {
            Ok(())
        } else {
            Err(AppError::Other(format!(
                "创建 WebDAV 远端目录失败: HTTP {}",
                status
            )))
        }
    }

    /// 上传文件 (PUT)
    pub async fn put_file(&self, rel_path: &str, content: Vec<u8>) -> AppResult<()> {
        let url = self.build_full_url(rel_path);
        let resp = self
            .client
            .put(&url)
            .basic_auth(&self.config.username, Some(&self.config.password))
            .body(content)
            .send()
            .await
            .map_err(|e| AppError::Other(format!("WebDAV PUT 上传失败: {}", e)))?;

        let status = resp.status();
        if status.is_success() || status == StatusCode::CREATED || status == StatusCode::NO_CONTENT
        {
            Ok(())
        } else {
            Err(AppError::Other(format!("WebDAV 上传失败: HTTP {}", status)))
        }
    }

    /// 下载文件 (GET)
    pub async fn get_file(&self, rel_path: &str) -> AppResult<Option<Vec<u8>>> {
        let url = self.build_full_url(rel_path);
        let resp = self
            .client
            .get(&url)
            .basic_auth(&self.config.username, Some(&self.config.password))
            .send()
            .await
            .map_err(|e| AppError::Other(format!("WebDAV GET 下载失败: {}", e)))?;

        let status = resp.status();
        if status == StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if !status.is_success() {
            return Err(AppError::Other(format!(
                "WebDAV 下载文件失败: HTTP {}",
                status
            )));
        }

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| AppError::Other(format!("读取 WebDAV 响应数据流失败: {}", e)))?;

        Ok(Some(bytes.to_vec()))
    }

    /// 删除文件 (DELETE)
    pub async fn delete_file(&self, rel_path: &str) -> AppResult<()> {
        let url = self.build_full_url(rel_path);
        let resp = self
            .client
            .delete(&url)
            .basic_auth(&self.config.username, Some(&self.config.password))
            .send()
            .await
            .map_err(|e| AppError::Other(format!("WebDAV DELETE 失败: {}", e)))?;

        let status = resp.status();
        if status.is_success()
            || status == StatusCode::NOT_FOUND
            || status == StatusCode::NO_CONTENT
        {
            Ok(())
        } else {
            Err(AppError::Other(format!("WebDAV 删除失败: HTTP {}", status)))
        }
    }
}

/// WebDAV 凭据会通过 Basic Auth 发送，生产服务必须使用 HTTPS。
/// 仅允许明确的本机回环地址使用 HTTP，不能用字符串 contains 绕过。
fn validate_server_url(raw: &str) -> AppResult<()> {
    let url =
        Url::parse(raw.trim()).map_err(|e| AppError::Other(format!("WebDAV 地址无效: {}", e)))?;
    let host = url
        .host_str()
        .ok_or_else(|| AppError::Other("WebDAV 地址缺少主机名".into()))?;
    let loopback = host.eq_ignore_ascii_case("localhost")
        || matches!(url.host(), Some(url::Host::Ipv4(ip)) if ip.is_loopback())
        || matches!(url.host(), Some(url::Host::Ipv6(ip)) if ip.is_loopback());
    if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
        return Err(AppError::Security(
            "WebDAV 必须使用 HTTPS；仅允许 localhost/回环地址使用 HTTP".into(),
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(AppError::Security("WebDAV 地址不得内嵌用户名或密码".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(server_url: &str) -> WebDavConfig {
        WebDavConfig {
            enabled: true,
            server_url: server_url.to_string(),
            username: "user".into(),
            password: "secret".into(),
            remote_dir: "/browsory".into(),
        }
    }

    #[test]
    fn rejects_plaintext_remote_webdav() {
        assert!(WebDavClient::new(config("http://dav.example.com/dav")).is_err());
        assert!(WebDavClient::new(config("http://localhost:8080/dav")).is_ok());
        assert!(WebDavClient::new(config("http://localhost.evil.example/dav")).is_err());
        assert!(WebDavClient::new(config("https://dav.example.com/dav")).is_ok());
        assert!(WebDavClient::new(config("https://localhost.evil.example/dav")).is_ok());
    }
}
