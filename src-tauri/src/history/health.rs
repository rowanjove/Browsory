use rusqlite::{params, Connection};
use std::time::Duration;
use url::Url;

use crate::database::models::LinkHealthStatus;
use crate::error::{AppError, AppResult};

pub fn build_wayback_url(url: &str) -> String {
    format!("https://web.archive.org/web/*/{}", url)
}

/// Literal/local-name policy only: never resolve DNS while filtering history.
pub(crate) fn is_non_public_url(raw_url: &str) -> bool {
    let Ok(parsed) = Url::parse(raw_url) else {
        return false;
    };
    match parsed.host() {
        Some(url::Host::Ipv4(ip)) => is_ip_restricted(&std::net::IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) => is_ip_restricted(&std::net::IpAddr::V6(ip)),
        Some(url::Host::Domain(host)) => {
            host.eq_ignore_ascii_case("localhost")
                || host.to_ascii_lowercase().ends_with(".localhost")
                || host.to_ascii_lowercase().ends_with(".local")
        }
        None => false,
    }
}

pub fn is_ip_restricted(ip: &std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(ipv4) => {
            let oct = ipv4.octets();
            ipv4.is_loopback()
                || ipv4.is_private()
                || ipv4.is_link_local()
                || ipv4.is_broadcast()
                || ipv4.is_unspecified()
                || oct[0] == 0 // 0.0.0.0/8
                || (oct[0] == 100 && (oct[1] & 0xC0) == 64) // 100.64.0.0/10 CGNAT
                || (oct[0] == 192 && oct[1] == 0 && oct[2] == 0) // 192.0.0.0/24
                || (oct[0] == 192 && oct[1] == 0 && oct[2] == 2) // 192.0.2.0/24 TEST-NET-1
                || (oct[0] == 198 && (oct[1] == 18 || oct[1] == 19)) // 198.18.0.0/15
                || (oct[0] == 198 && oct[1] == 51 && oct[2] == 100) // 198.51.100.0/24 TEST-NET-2
                || (oct[0] == 203 && oct[1] == 0 && oct[2] == 113) // 203.0.113.0/24 TEST-NET-3
                || oct[0] >= 224 // 224.0.0.0/4 Multicast & 240.0.0.0/4 Reserved
        }
        std::net::IpAddr::V6(ipv6) => {
            if ipv6.is_loopback() || ipv6.is_unspecified() {
                return true;
            }
            if let Some(ipv4) = ipv6.to_ipv4_mapped() {
                return is_ip_restricted(&std::net::IpAddr::V4(ipv4));
            }
            let seg = ipv6.segments();
            // Only global-unicast IPv6 (2000::/3) is eligible for probing.
            // This fail-closed boundary rejects multicast, site-local,
            // documentation, benchmark, and other reserved/non-global ranges.
            if (seg[0] & 0xe000) != 0x2000 {
                return true;
            }
            // Unique Local (fc00::/7)
            if (seg[0] & 0xfe00) == 0xfc00 {
                return true;
            }
            // Link-Local (fe80::/10)
            if (seg[0] & 0xffc0) == 0xfe80 {
                return true;
            }
            // Documentation (2001:db8::/32)
            if seg[0] == 0x2001 && seg[1] == 0xdb8 {
                return true;
            }
            // IETF special-use/documentation and benchmarking ranges.
            if seg[0] == 0x2001 && (seg[1] == 0x0002 || (seg[1] & 0xfff0) == 0x0010) {
                return true;
            }
            false
        }
    }
}

pub fn is_safe_public_url(raw_url: &str) -> Result<Url, String> {
    let parsed = Url::parse(raw_url).map_err(|e| format!("无效的 URL 格式: {}", e))?;
    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(format!("不支持的协议类型 '{}'，仅允许 http/https", scheme));
    }

    match parsed.host() {
        Some(url::Host::Domain(host)) => {
            let host_lower = host.to_lowercase();
            if host_lower == "localhost"
                || host_lower.ends_with(".localhost")
                || host_lower.ends_with(".local")
                || host_lower.ends_with(".internal")
                || host_lower.ends_with(".onion")
            {
                return Err("禁止探测本地与内网保留域名 (SSRF 防护)".to_string());
            }
        }
        Some(url::Host::Ipv4(ipv4)) => {
            if is_ip_restricted(&std::net::IpAddr::V4(ipv4)) {
                return Err("禁止探测本地回环、私有局域网或保留 IP 地址 (SSRF 防护)".to_string());
            }
        }
        Some(url::Host::Ipv6(ipv6)) => {
            if is_ip_restricted(&std::net::IpAddr::V6(ipv6)) {
                return Err("禁止探测本地回环、私有局域网或保留 IPv6 地址 (SSRF 防护)".to_string());
            }
        }
        None => {
            return Err("URL 缺少有效主机名".to_string());
        }
    }

    Ok(parsed)
}

fn resolve_hostname_ips(host: &str, port: u16) -> Result<Vec<std::net::SocketAddr>, String> {
    let addr_str = format!("{}:{}", host, port);
    match std::net::ToSocketAddrs::to_socket_addrs(&addr_str.as_str()) {
        Ok(addrs) => {
            let mut resolved = false;
            let mut safe_addrs = Vec::new();
            for addr in addrs {
                resolved = true;
                if is_ip_restricted(&addr.ip()) {
                    return Err(format!(
                        "域名 '{}' 解析到了私有/回环受限 IP {} (SSRF 防护)",
                        host,
                        addr.ip()
                    ));
                }
                safe_addrs.push(addr);
            }
            if !resolved {
                return Err(format!("域名 '{}' 无法解析到任何有效 IP 地址", host));
            }
            Ok(safe_addrs)
        }
        Err(e) => Err(format!("域名 '{}' DNS 解析失败: {}", host, e)),
    }
}

async fn resolve_with_timeout(
    timeout: Duration,
    work: impl FnOnce() -> Result<Vec<std::net::SocketAddr>, String> + Send + 'static,
) -> Result<Vec<std::net::SocketAddr>, String> {
    match tokio::time::timeout(timeout, tauri::async_runtime::spawn_blocking(work)).await {
        Ok(Ok(result)) => result,
        Ok(Err(error)) => Err(format!("DNS 工作任务失败: {error}")),
        Err(_) => Err("DNS 解析超时，请重试".into()),
    }
}

pub async fn probe_url_status_for_operation(
    url: &str,
    operation: &crate::security::session::VaultOperation,
) -> (Option<u16>, bool, Option<String>) {
    if let Err(error) = operation.check() {
        return (None, false, Some(error.to_string()));
    }
    let result = probe_url_status(url).await;
    if let Err(error) = operation.check() {
        return (None, false, Some(error.to_string()));
    }
    result
}

pub async fn probe_url_status(url: &str) -> (Option<u16>, bool, Option<String>) {
    let parsed = match is_safe_public_url(url) {
        Ok(p) => p,
        Err(err_msg) => return (None, false, Some(err_msg)),
    };

    let port = parsed.port_or_known_default().unwrap_or(80);
    let resolved = match parsed.host() {
        Some(url::Host::Ipv4(ip)) => Ok(vec![std::net::SocketAddr::new(ip.into(), port)]),
        Some(url::Host::Ipv6(ip)) => Ok(vec![std::net::SocketAddr::new(ip.into(), port)]),
        Some(url::Host::Domain(host)) => {
            // The OS resolver cannot be forcibly cancelled. Its detached worker
            // owns only a host and port: never a URL/path, vault key, operation
            // or connection. Cancelling the parent prevents late results from
            // reaching HTTP/SQL, and vault close never waits for OS DNS.
            let host = host.to_owned();
            resolve_with_timeout(Duration::from_secs(6), move || {
                resolve_hostname_ips(&host, port)
            })
            .await
        }
        None => Err("URL 缺少有效主机名".into()),
    };
    let resolved_addrs = match resolved {
        Ok(addrs) => addrs,
        Err(error) => return (None, false, Some(error)),
    };
    let host = match parsed.host_str() {
        Some(host) => host,
        None => return (None, false, Some("URL 缺少有效主机名".to_string())),
    };

    let client_res = reqwest::Client::builder()
        .timeout(Duration::from_secs(6))
        // Pin the pre-validated addresses and avoid proxy/DNS re-resolution.
        .no_proxy()
        .resolve_to_addrs(host, &resolved_addrs)
        // Following redirects would require pinning every target's transport
        // address. Treat redirects as non-followable to remain fail-closed.
        .redirect(reqwest::redirect::Policy::none())
        .build();

    let client = match client_res {
        Ok(c) => c,
        Err(e) => return (None, false, Some(e.to_string())),
    };

    let response = match client.head(url).send().await {
        Ok(response) if !matches!(response.status().as_u16(), 405 | 501) => Ok(response),
        _ => client.get(url).send().await,
    };
    match response {
        Ok(response) => {
            let code = response.status().as_u16();
            (Some(code), code < 400, None)
        }
        Err(error) => (None, false, Some(error.to_string())),
    }
}

pub fn get_url_health(conn: &Connection, url_id: i64) -> AppResult<Option<LinkHealthStatus>> {
    let res = conn.query_row(
        r#"
        SELECT lh.url_id, u.url, lh.status_code, lh.is_alive, lh.last_checked_at, lh.error_message
        FROM link_health lh
        JOIN urls u ON lh.url_id = u.id
        WHERE lh.url_id = ?1
        "#,
        params![url_id],
        |row| {
            let url_id: i64 = row.get(0)?;
            let url: String = row.get(1)?;
            let code_i64: Option<i64> = row.get(2)?;
            let is_alive_i64: i64 = row.get(3)?;
            let last_checked_at: i64 = row.get(4)?;
            let error_message: Option<String> = row.get(5)?;
            let wayback_url = build_wayback_url(&url);

            Ok(LinkHealthStatus {
                url_id,
                url,
                status_code: code_i64.map(|c| c as u16),
                is_alive: is_alive_i64 == 1,
                last_checked_at,
                error_message,
                wayback_url,
            })
        },
    );

    match res {
        Ok(s) => Ok(Some(s)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(AppError::from(e)),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn dns_wait_is_bounded_and_cannot_hold_a_vault_open() {
        use crate::security::vault::{VaultManager, VaultStatus};
        use std::sync::{mpsc, Arc};
        let root =
            std::env::temp_dir().join(format!("browsory_dns_cancel_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let vault = Arc::new(VaultManager::unlocked_fixture(root.clone()));
        let operation = vault.begin_operation().unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let request = std::thread::spawn(move || {
            tauri::async_runtime::block_on(async {
                operation
                    .run(async {
                        resolve_with_timeout(Duration::from_secs(6), move || {
                            started_tx.send(()).unwrap();
                            release_rx.recv().unwrap();
                            finished_tx.send(()).unwrap();
                            Ok(Vec::new())
                        })
                        .await
                    })
                    .await
            })
        });
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let closing = vault.clone();
        let (locked_tx, locked_rx) = mpsc::channel();
        let locker = std::thread::spawn(move || {
            locked_tx.send(closing.lock()).unwrap();
        });
        let locked = locked_rx.recv_timeout(Duration::from_secs(2));
        // Always release the synthetic resolver, even if a regression blocks lock.
        release_tx.send(()).unwrap();
        finished_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        locker.join().unwrap();
        assert!(request.join().unwrap().is_err());
        assert!(locked.unwrap().is_ok());
        assert_eq!(vault.status(), VaultStatus::Locked);
        assert!(vault.master_key().is_none());
        drop(vault);
        std::fs::remove_dir_all(root).unwrap();

        let (release_tx, release_rx) = mpsc::channel();
        let result = tauri::async_runtime::block_on(async {
            resolve_with_timeout(Duration::from_millis(20), move || {
                let _ = release_rx.recv();
                Ok(Vec::new())
            })
            .await
        });
        release_tx.send(()).unwrap();
        assert!(result.unwrap_err().contains("超时"));
    }

    use super::*;

    #[test]
    fn test_ip_restriction() {
        use std::net::IpAddr;

        // Loopback & Private & Link-local IPv4
        assert!(is_ip_restricted(&"127.0.0.1".parse::<IpAddr>().unwrap()));
        assert!(is_ip_restricted(&"10.0.1.2".parse::<IpAddr>().unwrap()));
        assert!(is_ip_restricted(&"172.16.0.1".parse::<IpAddr>().unwrap()));
        assert!(is_ip_restricted(&"192.168.1.1".parse::<IpAddr>().unwrap()));
        assert!(is_ip_restricted(
            &"169.254.169.254".parse::<IpAddr>().unwrap()
        ));
        assert!(is_ip_restricted(&"100.64.0.1".parse::<IpAddr>().unwrap()));
        assert!(is_ip_restricted(&"0.0.0.0".parse::<IpAddr>().unwrap()));

        // Loopback & Unique Local & Link-local IPv6
        assert!(is_ip_restricted(&"::1".parse::<IpAddr>().unwrap()));
        assert!(is_ip_restricted(&"::".parse::<IpAddr>().unwrap()));
        assert!(is_ip_restricted(&"fc00::1".parse::<IpAddr>().unwrap()));
        assert!(is_ip_restricted(&"fe80::1".parse::<IpAddr>().unwrap()));
        assert!(is_ip_restricted(&"ff02::1".parse::<IpAddr>().unwrap()));
        assert!(is_ip_restricted(&"fec0::1".parse::<IpAddr>().unwrap()));
        assert!(is_ip_restricted(&"100::1".parse::<IpAddr>().unwrap()));

        // Public IPs should NOT be restricted
        assert!(!is_ip_restricted(&"8.8.8.8".parse::<IpAddr>().unwrap()));
        assert!(!is_ip_restricted(&"1.1.1.1".parse::<IpAddr>().unwrap()));
        assert!(!is_ip_restricted(
            &"2606:4700:4700::1111".parse::<IpAddr>().unwrap()
        ));
    }

    #[test]
    fn test_ssrf_protection() {
        // Unsafe URLs that must be blocked
        assert!(is_safe_public_url("http://localhost:8080").is_err());
        assert!(is_safe_public_url("http://127.0.0.1/admin").is_err());
        assert!(is_safe_public_url("http://[::1]:3000/").is_err());
        assert!(is_safe_public_url("http://[ff02::1]/").is_err());
        assert!(is_safe_public_url("http://192.168.1.1/router").is_err());
        assert!(is_safe_public_url("http://10.0.0.5/service").is_err());
        assert!(is_safe_public_url("http://169.254.169.254/latest/meta-data/").is_err());
        assert!(is_safe_public_url("ftp://example.com/file").is_err());
        assert!(is_safe_public_url("file:///etc/passwd").is_err());
        assert!(is_safe_public_url("http://company.internal/api").is_err());
        assert!(is_safe_public_url("http://service.local").is_err());
        assert!(is_safe_public_url("http://hidden.onion").is_err());

        // Safe public URLs that should be allowed
        assert!(is_safe_public_url("https://github.com/rust-lang/rust").is_ok());
        assert!(is_safe_public_url("http://example.com").is_ok());
        assert!(is_safe_public_url("https://docs.rs/rusqlite").is_ok());
    }

    #[test]
    fn test_probe_url_status_ssrf_blocked() {
        tauri::async_runtime::block_on(async {
            let (code, alive, err) = probe_url_status("http://127.0.0.1:8080/metrics").await;
            assert_eq!(code, None);
            assert!(!alive);
            assert!(err.is_some());
            assert!(err.unwrap().contains("SSRF"));

            let (code, alive, err) =
                probe_url_status("http://169.254.169.254/latest/meta-data").await;
            assert_eq!(code, None);
            assert!(!alive);
            assert!(err.is_some());
            assert!(err.unwrap().contains("SSRF"));
        });
    }
}
