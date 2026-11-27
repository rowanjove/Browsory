use sha2::{Digest, Sha256};
use std::cmp::min;

/// Shared upper bound for AI success and error response bodies.
///
/// This is deliberately applied before JSON parsing so an untrusted endpoint
/// cannot make the process allocate an unbounded response buffer.
pub const MAX_AI_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

/// Returns a stable, non-secret keyring name bound to one endpoint.
///
/// The endpoint itself is never stored in the credential store key. SHA-256 is
/// used here only as a deterministic namespace (not as a secret or an
/// authenticity mechanism); the secret value remains in the OS-backed store.
pub fn credential_ref_for_endpoint(base_url: &str, embedding: bool) -> String {
    let normalized = base_url.trim().trim_end_matches('/');
    let hash = Sha256::digest(normalized.as_bytes());
    let prefix = if embedding {
        "ai_embedding_credential_"
    } else {
        "ai_credential_"
    };
    format!("{}{}", prefix, hex::encode(hash))
}

/// User-configured AI requests may not forward prompts or credentials to a
/// redirect destination that has not been selected and validated by the user.
pub fn ai_client(
    base_url: &str,
    timeout: std::time::Duration,
) -> Result<reqwest::Client, reqwest::Error> {
    let builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout);
    let builder = if crate::commands::settings::is_local_ai_endpoint(base_url) {
        builder.no_proxy()
    } else {
        builder
    };
    builder.build()
}

pub async fn read_response_body_limited(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, String> {
    if let Some(length) = response.content_length() {
        if length > limit as u64 {
            return Err(format!("响应体超过大小上限 ({} 字节)", limit));
        }
    }

    let initial_capacity = response
        .content_length()
        .and_then(|length| usize::try_from(length).ok())
        .map(|length| min(length, limit))
        .unwrap_or(0);
    let mut body = Vec::with_capacity(initial_capacity);

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("读取响应数据流失败: {}", e))?
    {
        if chunk.len() > limit.saturating_sub(body.len()) {
            return Err(format!("响应体超过大小上限 ({} 字节)", limit));
        }
        body.extend_from_slice(&chunk);
    }

    Ok(body)
}

pub async fn read_response_text_limited(
    response: reqwest::Response,
    limit: usize,
) -> Result<String, String> {
    let body = read_response_body_limited(response, limit).await?;
    Ok(String::from_utf8_lossy(&body).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_client_does_not_forward_post_body_or_auth_to_redirect_targets() {
        for status in [301, 302, 307, 308] {
            let target = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            target.set_nonblocking(true).unwrap();
            let destination = format!("http://{}/unexpected", target.local_addr().unwrap());
            let origin = crate::ai::test_http::FixtureServer::start(format!(
                "HTTP/1.1 {status} Redirect\r\nLocation: {destination}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            ));
            let response = tauri::async_runtime::block_on(async {
                ai_client(&origin.base_url, std::time::Duration::from_secs(3))
                    .unwrap()
                    .post(&origin.base_url)
                    .header("Authorization", "Bearer synthetic-secret-token")
                    .body("synthetic private prompt")
                    .send()
                    .await
                    .unwrap()
            });
            assert_eq!(response.status().as_u16(), status);
            assert!(
                matches!(target.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
            );
        }
    }

    #[test]
    fn response_limit_checks_declared_and_streamed_sizes() {
        use std::io::{Read, Write};
        for (response, allowed) in [
            ("HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\n1234", true),
            ("HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\n12345", false),
            ("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n3\r\n123\r\n3\r\n456\r\n0\r\n\r\n", false),
        ] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = format!("http://{}/fixture", listener.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
                let mut request = Vec::new();
                let mut chunk = [0u8; 1024];
                while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                    let count = stream.read(&mut chunk).unwrap();
                    assert!(count != 0, "fixture request ended before headers");
                    request.extend_from_slice(&chunk[..count]);
                    assert!(request.len() <= 16 * 1024, "fixture headers exceed bound");
                }
                let _ = stream.write_all(response.as_bytes());
            });
            let result = tauri::async_runtime::block_on(async {
                let client = reqwest::Client::builder().no_proxy().timeout(std::time::Duration::from_secs(5)).build().unwrap();
                let response = client.get(endpoint).send().await.unwrap();
                read_response_body_limited(response, 4).await
            });
            assert_eq!(result.is_ok(), allowed);
            if let Ok(bytes) = result { assert_eq!(bytes, b"1234"); }
            server.join().unwrap();
        }
    }

    #[test]
    fn credential_ref_is_stable_and_scope_specific() {
        assert_eq!(
            credential_ref_for_endpoint("https://api.example/v1///", false),
            credential_ref_for_endpoint(" https://api.example/v1 ", false)
        );
        assert_ne!(
            credential_ref_for_endpoint("https://api.example/v1", false),
            credential_ref_for_endpoint("https://api.example/v1", true)
        );
        assert!(credential_ref_for_endpoint("https://api.example/v1", false)
            .starts_with("ai_credential_"));
        assert_eq!(
            credential_ref_for_endpoint("https://api.example/v1", false),
            "ai_credential_ca596ae86d2e18746b0c83440bfa88150cb12b64e2a32554882cdfb442c2fd69"
        );
    }
}
