use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use hkdf::Hkdf;
use rand::RngCore;
use sha2::Sha256;
use std::io::{Read, Write};

use crate::error::{AppError, AppResult};

const HKDF_SYNC_SALT: &[u8] = b"browsory-e2ee-sync-salt-v1";
const HKDF_SYNC_INFO: &[u8] = b"browsory-webdav-sync-encryption-key";
const NONCE_SIZE: usize = 12;

/// 为未来双向同步协议提供 HKDF-SHA256 密钥派生 helper。
/// 当前 WebDAV 双向同步在命令层 fail-closed，避免把单向密文上传冒充同步。
pub fn derive_sync_key(master_key_str: &str) -> AppResult<[u8; 32]> {
    let hk = Hkdf::<Sha256>::new(Some(HKDF_SYNC_SALT), master_key_str.as_bytes());
    let mut okm = [0u8; 32];
    hk.expand(HKDF_SYNC_INFO, &mut okm)
        .map_err(|_| AppError::Other("HKDF 密钥派生失败".into()))?;
    Ok(okm)
}

/// 流程：明文 JSON -> Gzip 压缩 -> AES-256-GCM 加密 (含 12 字节 Nonce)。
pub fn compress_and_encrypt(plaintext: &[u8], key_bytes: &[u8; 32]) -> AppResult<Vec<u8>> {
    // 1. Gzip 压缩
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(plaintext)?;
    let compressed = encoder.finish()?;

    // 2. 生成 12 字节随机 Nonce
    let mut nonce_bytes = [0u8; NONCE_SIZE];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    // 3. AES-256-GCM 加密
    let key = Key::<Aes256Gcm>::from_slice(key_bytes);
    let cipher = Aes256Gcm::new(key);
    let ciphertext = cipher
        .encrypt(nonce, compressed.as_ref())
        .map_err(|e| AppError::Other(format!("AES-GCM 加密失败: {}", e)))?;

    // 4. 组装输出：Nonce (12B) + 密文
    let mut output = Vec::with_capacity(NONCE_SIZE + ciphertext.len());
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(&ciphertext);

    Ok(output)
}

/// 流程：密文 (含 Nonce) -> AES-256-GCM 解密 -> Gzip 解压缩 -> 明文
pub fn decrypt_and_decompress(data: &[u8], key_bytes: &[u8; 32]) -> AppResult<Vec<u8>> {
    if data.len() < NONCE_SIZE {
        return Err(AppError::Other("加密同步包数据过短，无法解析 Nonce".into()));
    }

    let (nonce_bytes, ciphertext) = data.split_at(NONCE_SIZE);
    let nonce = Nonce::from_slice(nonce_bytes);

    // 1. AES-256-GCM 解密
    let key = Key::<Aes256Gcm>::from_slice(key_bytes);
    let cipher = Aes256Gcm::new(key);
    let decompressed_encrypted = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| AppError::Other("同步包解密验签失败，可能同步密码不一致或数据损坏".into()))?;

    // 2. Gzip 解压
    let mut decoder = GzDecoder::new(&decompressed_encrypted[..]);
    let mut plaintext = Vec::new();
    decoder.read_to_end(&mut plaintext)?;

    Ok(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_crypto_roundtrip() {
        let master_key = "test-master-key-123456";
        let sync_key = derive_sync_key(master_key).unwrap();

        let original_data =
            r#"{"action":"sync_bookmarks","items":[1,2,3,4],"note":"端到端加密数据"}"#.as_bytes();

        let encrypted_blob = compress_and_encrypt(original_data, &sync_key).unwrap();
        assert_ne!(encrypted_blob, original_data);

        let decrypted = decrypt_and_decompress(&encrypted_blob, &sync_key).unwrap();
        assert_eq!(decrypted, original_data);
    }

    #[test]
    fn test_sync_crypto_wrong_key_fails() {
        let key1 = derive_sync_key("master-key-A").unwrap();
        let key2 = derive_sync_key("master-key-B").unwrap();

        let data = b"sensitive payload";
        let encrypted = compress_and_encrypt(data, &key1).unwrap();

        // 用 key2 解密应失败
        assert!(decrypt_and_decompress(&encrypted, &key2).is_err());
    }
}
