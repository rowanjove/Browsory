use chrono::Utc;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

use crate::error::{AppError, AppResult};

/// 官方内置公钥 (32 bytes Hex / Base64)
/// 用于在客户端完全离线校验 License 签名
pub const OFFICIAL_LICENSE_PUBKEY_HEX: &str =
    "a8f7c5e219b4e05b9b1d3d6e5a4f7832e18b9560f38c3127845610ec87fa673e";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LicenseCertificate {
    pub license_id: String,
    pub licensee_name: String,
    #[serde(default)]
    pub licensee_email: Option<String>,
    pub plan: String, // "pro" | "lifetime"
    pub issued_at: i64,
    pub expires_at: Option<i64>, // None 表示永久
    pub device_limit: u32,
    pub features: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseInfo {
    pub is_pro: bool,
    pub plan: String,
    pub licensee_name: Option<String>,
    pub licensee_email: Option<String>,
    pub expires_at: Option<i64>,
    pub is_expired: bool,
    pub device_limit: u32,
    pub features: Vec<String>,
}

impl Default for LicenseInfo {
    fn default() -> Self {
        Self {
            is_pro: false,
            plan: "core".to_string(),
            licensee_name: None,
            licensee_email: None,
            expires_at: None,
            is_expired: false,
            device_limit: 1,
            features: vec![
                "history_import".into(),
                "fts_search".into(),
                "basic_filter".into(),
                "basic_export".into(),
            ],
        }
    }
}

/// 验证 License Key 签名并解析出证书
/// Key 格式：<Base64 Payload>.<Hex Signature>
pub fn verify_license_key(license_key: &str, pubkey_hex: &str) -> AppResult<LicenseCertificate> {
    let parts: Vec<&str> = license_key.trim().split('.').collect();
    if parts.len() != 2 {
        return Err(AppError::Other("无效的授权码格式 (缺少签名或载荷)".into()));
    }

    let payload_b64 = parts[0];
    let sig_hex = parts[1];

    let payload_bytes = base64_decode(payload_b64)
        .map_err(|e| AppError::Other(format!("授权码载荷解码失败: {}", e)))?;

    let sig_bytes =
        hex::decode(sig_hex).map_err(|e| AppError::Other(format!("授权码签名无效: {}", e)))?;

    if sig_bytes.len() != 64 {
        return Err(AppError::Other("授权码签名长度异常".into()));
    }

    let pubkey_bytes =
        hex::decode(pubkey_hex).map_err(|e| AppError::Other(format!("公钥格式异常: {}", e)))?;

    if pubkey_bytes.len() != 32 {
        return Err(AppError::Other("公钥长度异常".into()));
    }

    let verifying_key = VerifyingKey::from_bytes(pubkey_bytes.as_slice().try_into().unwrap())
        .map_err(|e| AppError::Other(format!("构造验签公钥失败: {}", e)))?;

    let signature = Signature::from_bytes(sig_bytes.as_slice().try_into().unwrap());

    verifying_key
        .verify(&payload_bytes, &signature)
        .map_err(|_| AppError::Other("授权签名校验未通过，该授权码无效或已被篡改".into()))?;

    let cert: LicenseCertificate = serde_json::from_slice(&payload_bytes)
        .map_err(|e| AppError::Other(format!("授权证书内容反序列化失败: {}", e)))?;

    // 检查有效期
    let now = Utc::now().timestamp_millis();
    if let Some(expires_at) = cert.expires_at {
        if now > expires_at {
            return Err(AppError::Other("当前授权已超过有效期".into()));
        }
    }

    Ok(cert)
}

fn get_license_file_path(app_dir: &Path) -> PathBuf {
    app_dir.join("license.key")
}

/// 保存并激活授权码
pub fn save_license_key(app_dir: &Path, license_key: &str) -> AppResult<LicenseCertificate> {
    let cert = verify_license_key(license_key, OFFICIAL_LICENSE_PUBKEY_HEX)?;
    let target_path = get_license_file_path(app_dir);
    fs::write(&target_path, license_key.trim())?;
    info!("已成功激活并保存 Browsory Pro 授权: {}", cert.licensee_name);
    Ok(cert)
}

/// 读取并校验当前本地激活状态
pub fn load_active_license(app_dir: &Path) -> LicenseInfo {
    let path = get_license_file_path(app_dir);
    if !path.exists() {
        return LicenseInfo::default();
    }

    match fs::read_to_string(&path) {
        Ok(content) => match verify_license_key(&content, OFFICIAL_LICENSE_PUBKEY_HEX) {
            Ok(cert) => {
                let now = Utc::now().timestamp_millis();
                let is_expired = cert.expires_at.map(|exp| now > exp).unwrap_or(false);
                LicenseInfo {
                    is_pro: !is_expired,
                    plan: cert.plan,
                    licensee_name: Some(cert.licensee_name),
                    licensee_email: cert.licensee_email,
                    expires_at: cert.expires_at,
                    is_expired,
                    device_limit: cert.device_limit,
                    features: cert.features,
                }
            }
            Err(err) => {
                warn!("本地 License 校验失败: {}", err);
                LicenseInfo::default()
            }
        },
        Err(err) => {
            warn!("读取本地 License 失败: {}", err);
            LicenseInfo::default()
        }
    }
}

pub const FEATURE_SEMANTIC_SEARCH: &str = "semantic_search";
pub const FEATURE_OFFLINE_ARCHIVE: &str = "snapshots";
pub const FEATURE_WEBDAV_SYNC: &str = "sync";

/// 后端功能门禁。证书未列出 features 时，Pro 视为开通全部专业功能。
pub fn require_pro(app_dir: &Path, feature: &str) -> AppResult<()> {
    let info = load_active_license(app_dir);
    if !info.is_pro {
        return Err(AppError::Other(format!(
            "此功能需要 Browsory Pro 授权（{}）",
            feature
        )));
    }
    if info.features.is_empty()
        || info
            .features
            .iter()
            .any(|item| item == feature || item == "*")
    {
        return Ok(());
    }
    Err(AppError::Other(format!(
        "当前授权未包含功能：{}",
        feature
    )))
}

/// 移除本地授权，恢复免费 Core 版
pub fn revoke_license(app_dir: &Path) -> AppResult<()> {
    let path = get_license_file_path(app_dir);
    if path.exists() {
        let _ = fs::remove_file(&path);
    }
    info!("已移除本地 License 授权");
    Ok(())
}

fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    // 简易 RFC4648 standard base64 decoder
    const B64_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut table = [255u8; 256];
    for (i, &c) in B64_CHARS.iter().enumerate() {
        table[c as usize] = i as u8;
    }

    let clean: Vec<u8> = input
        .bytes()
        .filter(|&b| b != b'=' && !b.is_ascii_whitespace())
        .collect();
    let mut output = Vec::with_capacity(clean.len() * 3 / 4);

    for chunk in clean.chunks(4) {
        let mut b = [0u32; 4];
        for i in 0..chunk.len() {
            let val = table[chunk[i] as usize];
            if val == 255 {
                return Err("Invalid base64 character".into());
            }
            b[i] = val as u32;
        }

        match chunk.len() {
            4 => {
                let n = (b[0] << 18) | (b[1] << 12) | (b[2] << 6) | b[3];
                output.push(((n >> 16) & 0xFF) as u8);
                output.push(((n >> 8) & 0xFF) as u8);
                output.push((n & 0xFF) as u8);
            }
            3 => {
                let n = (b[0] << 18) | (b[1] << 12) | (b[2] << 6);
                output.push(((n >> 16) & 0xFF) as u8);
                output.push(((n >> 8) & 0xFF) as u8);
            }
            2 => {
                let n = (b[0] << 18) | (b[1] << 12);
                output.push(((n >> 16) & 0xFF) as u8);
            }
            _ => return Err("Illegal base64 chunk".into()),
        }
    }

    Ok(output)
}

pub fn base64_encode(input: &[u8]) -> String {
    const B64_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };

        let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
        out.push(B64_CHARS[((n >> 18) & 63) as usize] as char);
        out.push(B64_CHARS[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(B64_CHARS[((n >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(B64_CHARS[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use ed25519_dalek::Signer;
    use ed25519_dalek::SigningKey;
    use rand::RngCore;

    #[test]
    fn test_license_verification_and_tampering() {
        // 生成一对测试密钥
        let mut seed = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut seed);
        let signing_key = SigningKey::from_bytes(&seed);
        let verifying_key = signing_key.verifying_key();
        let pubkey_hex = hex::encode(verifying_key.as_bytes());

        let cert = LicenseCertificate {
            license_id: "LIC-PRO-2026-TEST".into(),
            licensee_name: "测试用户".into(),
            licensee_email: Some("test@example.com".into()),
            plan: "pro".into(),
            issued_at: Utc::now().timestamp_millis(),
            expires_at: Some(Utc::now().timestamp_millis() + 365 * 86400 * 1000),
            device_limit: 3,
            features: vec!["semantic_search".into(), "snapshots".into(), "sync".into()],
        };

        let payload_json = serde_json::to_vec(&cert).unwrap();
        let sig = signing_key.sign(&payload_json);
        let license_key = format!(
            "{}.{}",
            base64_encode(&payload_json),
            hex::encode(sig.to_bytes())
        );

        // 验证合法签名
        let verified = verify_license_key(&license_key, &pubkey_hex).unwrap();
        assert_eq!(verified.licensee_name, "测试用户");
        assert_eq!(verified.plan, "pro");

        // 测试篡改载荷（改动1个字节）
        let tampered_key = format!("X{}", &license_key[1..]);
        assert!(verify_license_key(&tampered_key, &pubkey_hex).is_err());

        // 测试错误签名
        let wrong_sig_key = format!(
            "{}.{}",
            base64_encode(&payload_json),
            hex::encode([0u8; 64])
        );
        assert!(verify_license_key(&wrong_sig_key, &pubkey_hex).is_err());
    }

    #[test]
    fn test_license_expiration() {
        let mut seed = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut seed);
        let signing_key = SigningKey::from_bytes(&seed);
        let verifying_key = signing_key.verifying_key();
        let pubkey_hex = hex::encode(verifying_key.as_bytes());

        let expired_cert = LicenseCertificate {
            license_id: "LIC-EXPIRED".into(),
            licensee_name: "过期用户".into(),
            licensee_email: None,
            plan: "pro".into(),
            issued_at: 1000,
            expires_at: Some(2000), // 已过期
            device_limit: 1,
            features: vec![],
        };

        let payload_json = serde_json::to_vec(&expired_cert).unwrap();
        let sig = signing_key.sign(&payload_json);
        let license_key = format!(
            "{}.{}",
            base64_encode(&payload_json),
            hex::encode(sig.to_bytes())
        );

        let res = verify_license_key(&license_key, &pubkey_hex);
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("超过有效期"));
    }

    #[test]
    fn require_pro_rejects_missing_license() {
        let dir = std::env::temp_dir().join(format!(
            "browsory_license_gate_{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let _ = fs::create_dir_all(&dir);
        let err = require_pro(&dir, FEATURE_SEMANTIC_SEARCH).unwrap_err();
        assert!(err.to_string().contains("Pro 授权"));
        let _ = fs::remove_dir_all(&dir);
    }
}
