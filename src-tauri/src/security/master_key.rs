use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use rand::RngCore;
use serde::{Deserialize, Serialize};

use super::dpapi::dpapi_unprotect;

pub const ARGON2_MEM_COST_KB: u32 = 65536; // 64 MB
pub const ARGON2_TIME_COST: u32 = 3; // 3 iterations
pub const ARGON2_PARALLELISM: u32 = 4; // 4 threads

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MasterKeyEnvelope {
    pub version: u32,
    pub kdf_algorithm: String,
    pub pin_salt_hex: String,
    pub pin_kdf_mem_kb: u32,
    pub pin_kdf_time: u32,
    pub pin_kdf_lanes: u32,
    pub pin_nonce_hex: String,
    pub pin_ciphertext_hex: String,

    pub recovery_salt_hex: String,
    pub recovery_nonce_hex: String,
    pub recovery_ciphertext_hex: String,

    pub dpapi_protected: bool,
    pub dpapi_blob_hex: Option<String>,
    pub updated_at: i64,
}

/// Derives a 256-bit key from PIN using Argon2id
pub fn derive_argon2id_key(secret: &str, salt: &[u8]) -> Result<[u8; 32], String> {
    let params = Params::new(
        ARGON2_MEM_COST_KB,
        ARGON2_TIME_COST,
        ARGON2_PARALLELISM,
        Some(32),
    )
    .map_err(|e| format!("Argon2 params error: {}", e))?;

    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut derived_key = [0u8; 32];
    argon2
        .hash_password_into(secret.as_bytes(), salt, &mut derived_key)
        .map_err(|e| format!("Argon2 KDF computation failed: {}", e))?;

    Ok(derived_key)
}

/// Generates a standardized Recovery Key formatted as:
/// BHA-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX (32 hex characters grouped)
pub fn generate_recovery_key() -> (String, [u8; 32]) {
    let mut raw_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw_bytes);

    let hex_str = hex::encode(raw_bytes).to_uppercase();
    // Format into 8 chunks of 4 characters: BHA-1234-5678-...
    let mut chunks = Vec::new();
    chunks.push("BHA".to_string());
    for i in (0..hex_str.len()).step_by(4) {
        chunks.push(hex_str[i..i + 4].to_string());
    }
    (chunks.join("-"), raw_bytes)
}

/// Normalizes a user-input recovery key by stripping whitespace, dashes, and BHA prefix
pub fn normalize_recovery_key(input: &str) -> String {
    let upper = input.trim().to_uppercase();
    let stripped = upper.trim_start_matches("BHA").replace(['-', ' ', ':'], "");
    stripped
}

/// Creates a new Master Key envelope initialized with a new Master Key, PIN, and Recovery Key
pub fn create_master_key_envelope(
    pin: &str,
    now_ms: i64,
) -> Result<(MasterKeyEnvelope, String, [u8; 32]), String> {
    // 1. Generate 256-bit high-entropy Master Key
    let mut master_key = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut master_key);

    // 2. Generate Recovery Key
    let (recovery_key_str, _) = generate_recovery_key();
    let normalized_rec = normalize_recovery_key(&recovery_key_str);

    // 3. Setup PIN KEK
    let mut pin_salt = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut pin_salt);
    let pin_kek = derive_argon2id_key(pin, &pin_salt)?;

    let mut pin_nonce = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut pin_nonce);
    let pin_cipher =
        Aes256Gcm::new_from_slice(&pin_kek).map_err(|e| format!("Aes256Gcm init error: {}", e))?;
    let pin_ciphertext = pin_cipher
        .encrypt(Nonce::from_slice(&pin_nonce), master_key.as_ref())
        .map_err(|e| format!("Master Key PIN encryption failed: {}", e))?;

    // 4. Setup Recovery KEK
    let mut rec_salt = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut rec_salt);
    let rec_kek = derive_argon2id_key(&normalized_rec, &rec_salt)?;

    let mut rec_nonce = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut rec_nonce);
    let rec_cipher =
        Aes256Gcm::new_from_slice(&rec_kek).map_err(|e| format!("Aes256Gcm init error: {}", e))?;
    let rec_ciphertext = rec_cipher
        .encrypt(Nonce::from_slice(&rec_nonce), master_key.as_ref())
        .map_err(|e| format!("Master Key Recovery encryption failed: {}", e))?;

    // 5. Windows DPAPI envelope wrap
    let envelope = MasterKeyEnvelope {
        version: 1,
        kdf_algorithm: "argon2id".to_string(),
        pin_salt_hex: hex::encode(pin_salt),
        pin_kdf_mem_kb: ARGON2_MEM_COST_KB,
        pin_kdf_time: ARGON2_TIME_COST,
        pin_kdf_lanes: ARGON2_PARALLELISM,
        pin_nonce_hex: hex::encode(pin_nonce),
        pin_ciphertext_hex: hex::encode(pin_ciphertext),

        recovery_salt_hex: hex::encode(rec_salt),
        recovery_nonce_hex: hex::encode(rec_nonce),
        recovery_ciphertext_hex: hex::encode(rec_ciphertext),

        dpapi_protected: false,
        dpapi_blob_hex: None,
        updated_at: now_ms,
    };

    Ok((envelope, recovery_key_str, master_key))
}

/// Unwraps the Master Key from the envelope using the user's PIN
pub fn unwrap_master_key_with_pin(
    envelope: &MasterKeyEnvelope,
    pin: &str,
) -> Result<[u8; 32], String> {
    let pin_salt = hex::decode(&envelope.pin_salt_hex)
        .map_err(|_| "Invalid PIN salt in envelope".to_string())?;
    let pin_nonce = hex::decode(&envelope.pin_nonce_hex)
        .map_err(|_| "Invalid PIN nonce in envelope".to_string())?;
    let pin_ciphertext = hex::decode(&envelope.pin_ciphertext_hex)
        .map_err(|_| "Invalid PIN ciphertext in envelope".to_string())?;

    let pin_kek = derive_argon2id_key(pin, &pin_salt)?;
    let cipher =
        Aes256Gcm::new_from_slice(&pin_kek).map_err(|e| format!("Aes256Gcm init error: {}", e))?;

    let decrypted = cipher
        .decrypt(Nonce::from_slice(&pin_nonce), pin_ciphertext.as_ref())
        .map_err(|_| "PIN 校验失败，无法解密主密钥".to_string())?;

    if decrypted.len() != 32 {
        return Err("解密出的主密钥长度异常".to_string());
    }

    let mut master_key = [0u8; 32];
    master_key.copy_from_slice(&decrypted);
    Ok(master_key)
}

/// Unwraps the Master Key from the envelope using the Recovery Key
pub fn unwrap_master_key_with_recovery(
    envelope: &MasterKeyEnvelope,
    recovery_key: &str,
) -> Result<[u8; 32], String> {
    let normalized_rec = normalize_recovery_key(recovery_key);
    let rec_salt = hex::decode(&envelope.recovery_salt_hex)
        .map_err(|_| "Invalid recovery salt in envelope".to_string())?;
    let rec_nonce = hex::decode(&envelope.recovery_nonce_hex)
        .map_err(|_| "Invalid recovery nonce in envelope".to_string())?;
    let rec_ciphertext = hex::decode(&envelope.recovery_ciphertext_hex)
        .map_err(|_| "Invalid recovery ciphertext in envelope".to_string())?;

    let rec_kek = derive_argon2id_key(&normalized_rec, &rec_salt)?;
    let cipher =
        Aes256Gcm::new_from_slice(&rec_kek).map_err(|e| format!("Aes256Gcm init error: {}", e))?;

    let decrypted = cipher
        .decrypt(Nonce::from_slice(&rec_nonce), rec_ciphertext.as_ref())
        .map_err(|_| "恢复密钥无效，无法解密主密钥".to_string())?;

    if decrypted.len() != 32 {
        return Err("解密出的主密钥长度异常".to_string());
    }

    let mut master_key = [0u8; 32];
    master_key.copy_from_slice(&decrypted);
    Ok(master_key)
}

/// Re-wraps the Master Key with a new PIN without altering the underlying Master Key
pub fn rewrap_master_key_envelope(
    envelope: &mut MasterKeyEnvelope,
    master_key: &[u8; 32],
    new_pin: &str,
    now_ms: i64,
) -> Result<(), String> {
    let mut pin_salt = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut pin_salt);
    let pin_kek = derive_argon2id_key(new_pin, &pin_salt)?;

    let mut pin_nonce = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut pin_nonce);
    let pin_cipher =
        Aes256Gcm::new_from_slice(&pin_kek).map_err(|e| format!("Aes256Gcm init error: {}", e))?;
    let pin_ciphertext = pin_cipher
        .encrypt(Nonce::from_slice(&pin_nonce), master_key.as_ref())
        .map_err(|e| format!("Master Key re-encryption failed: {}", e))?;

    envelope.pin_salt_hex = hex::encode(pin_salt);
    envelope.pin_nonce_hex = hex::encode(pin_nonce);
    envelope.pin_ciphertext_hex = hex::encode(pin_ciphertext);
    envelope.updated_at = now_ms;

    Ok(())
}

/// Optional: unwrap master key purely using DPAPI (if available on local Windows user)
pub fn unwrap_master_key_with_dpapi(envelope: &MasterKeyEnvelope) -> Result<[u8; 32], String> {
    if !envelope.dpapi_protected {
        return Err("DPAPI protection not enabled for envelope".to_string());
    }
    let blob_hex = envelope
        .dpapi_blob_hex
        .as_ref()
        .ok_or_else(|| "Missing DPAPI blob in envelope".to_string())?;
    let blob = hex::decode(blob_hex).map_err(|e| format!("Invalid hex: {}", e))?;
    let decrypted = dpapi_unprotect(&blob)?;
    if decrypted.len() != 32 {
        return Err("DPAPI decrypted key length invalid".to_string());
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&decrypted);
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_master_key_roundtrip() {
        let pin = "123456";
        let now = 1700000000;
        let (mut envelope, recovery_key, original_key) =
            create_master_key_envelope(pin, now).expect("Envelope creation failed");

        // 1. Unwrap with correct PIN
        let unlocked_key =
            unwrap_master_key_with_pin(&envelope, pin).expect("Failed to unlock with PIN");
        assert_eq!(original_key, unlocked_key);

        // 2. Reject wrong PIN
        assert!(unwrap_master_key_with_pin(&envelope, "654321").is_err());

        // 3. Unwrap with Recovery Key
        let recovered_key = unwrap_master_key_with_recovery(&envelope, &recovery_key)
            .expect("Failed to unlock with recovery key");
        assert_eq!(original_key, recovered_key);

        // 4. Change PIN
        let new_pin = "8888";
        rewrap_master_key_envelope(&mut envelope, &original_key, new_pin, now + 10)
            .expect("Rewrap failed");

        // Old PIN fails, new PIN succeeds
        assert!(unwrap_master_key_with_pin(&envelope, pin).is_err());
        let re_unlocked =
            unwrap_master_key_with_pin(&envelope, new_pin).expect("Failed with new PIN");
        assert_eq!(original_key, re_unlocked);

        // Recovery key still works with original master key
        let recovered_after = unwrap_master_key_with_recovery(&envelope, &recovery_key)
            .expect("Recovery key failed after PIN change");
        assert_eq!(original_key, recovered_after);
    }
}
