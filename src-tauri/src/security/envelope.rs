use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use zeroize::{Zeroize, Zeroizing};

pub const ENVELOPE_CURRENT_VERSION: u32 = 1;
pub const KDF_ALGO_ARGON2ID: &str = "argon2id";

// Tuned Argon2id defaults (safe balance of defense and responsive unlock)
pub const DEFAULT_ARGON2_MEM_COST_KB: u32 = 65536; // 64 MB
pub const DEFAULT_ARGON2_TIME_COST: u32 = 3; // 3 iterations
pub const DEFAULT_ARGON2_PARALLELISM: u32 = 4; // 4 threads

// Resource constraints for untrusted metadata
pub const MIN_MEM_COST_KB: u32 = 16384; // 16 MB
pub const MAX_MEM_COST_KB: u32 = 262144; // 256 MB
pub const MIN_TIME_COST: u32 = 1;
pub const MAX_TIME_COST: u32 = 10;
pub const MIN_PARALLELISM: u32 = 1;
pub const MAX_PARALLELISM: u32 = 16;
pub const MIN_PASSWORD_LEN: usize = 12;
pub const MAX_ENVELOPE_BYTES: u64 = 64 * 1024;

pub const VAULT_ENVELOPE_FILE_NAME: &str = "vault.envelope.json";

/// Secure container for the 256-bit database master key that clears memory on drop.
#[derive(Clone)]
pub struct MasterKey(pub [u8; 32]);

impl MasterKey {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn generate() -> Self {
        use rand::RngCore;
        let mut key = Self([0u8; 32]);
        rand::rngs::OsRng.fill_bytes(&mut key.0);
        key
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Drop for MasterKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl std::fmt::Debug for MasterKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MasterKey(***REDACTED***)")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KdfParameters {
    pub mem_cost_kb: u32,
    pub time_cost: u32,
    pub parallelism: u32,
}

impl Default for KdfParameters {
    fn default() -> Self {
        Self {
            mem_cost_kb: DEFAULT_ARGON2_MEM_COST_KB,
            time_cost: DEFAULT_ARGON2_TIME_COST,
            parallelism: DEFAULT_ARGON2_PARALLELISM,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecoveryWrap {
    pub salt_hex: String,
    pub nonce_hex: String,
    pub ciphertext_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DpapiWrap {
    pub nonce_hex: String,
    pub ciphertext_hex: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultEnvelope {
    pub version: u32,
    pub kdf_algorithm: String,
    pub kdf_parameters: KdfParameters,
    pub password_salt_hex: String,
    pub password_nonce_hex: String,
    pub password_wrapped_key_hex: String,
    pub recovery_wrap: RecoveryWrap,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dpapi_wrap: Option<DpapiWrap>,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default = "legacy_recovery_confirmed")]
    pub recovery_key_confirmed: bool,
}

fn legacy_recovery_confirmed() -> bool {
    true
}

impl VaultEnvelope {
    /// Validates untrusted metadata fields before performing heavy crypto calculations.
    pub fn validate_constraints(&self) -> Result<(), String> {
        if self.version != ENVELOPE_CURRENT_VERSION {
            return Err(format!("不支持的保险库信封版本: {}", self.version));
        }
        if self.kdf_algorithm != KDF_ALGO_ARGON2ID {
            return Err(format!("不支持的 KDF 算法: {}", self.kdf_algorithm));
        }

        let p = &self.kdf_parameters;
        if p.mem_cost_kb < MIN_MEM_COST_KB || p.mem_cost_kb > MAX_MEM_COST_KB {
            return Err(format!(
                "KDF 内存参数超出安全范围 [{} - {} KB]: {} KB",
                MIN_MEM_COST_KB, MAX_MEM_COST_KB, p.mem_cost_kb
            ));
        }
        if p.time_cost < MIN_TIME_COST || p.time_cost > MAX_TIME_COST {
            return Err(format!(
                "KDF 迭代次数超出安全范围 [{} - {}]: {}",
                MIN_TIME_COST, MAX_TIME_COST, p.time_cost
            ));
        }
        if p.parallelism < MIN_PARALLELISM || p.parallelism > MAX_PARALLELISM {
            return Err(format!(
                "KDF 并行度超出安全范围 [{} - {}]: {}",
                MIN_PARALLELISM, MAX_PARALLELISM, p.parallelism
            ));
        }

        // Salt: 16 to 64 bytes -> 32 to 128 hex chars
        if self.password_salt_hex.len() < 32 || self.password_salt_hex.len() > 128 {
            return Err("口令盐长度异常".into());
        }
        if self.recovery_wrap.salt_hex.len() < 32 || self.recovery_wrap.salt_hex.len() > 128 {
            return Err("恢复密钥盐长度异常".into());
        }

        // Nonce: 12 bytes -> 24 hex chars
        if self.password_nonce_hex.len() != 24 {
            return Err("口令 Nonce 长度异常 (必须为 12 字节 / 24 hex)".into());
        }
        if self.recovery_wrap.nonce_hex.len() != 24 {
            return Err("恢复密钥 Nonce 长度异常 (必须为 12 字节 / 24 hex)".into());
        }

        // 32-byte key + 16-byte GCM tag = 48 bytes -> 96 hex chars
        if self.password_wrapped_key_hex.len() != 96 {
            return Err("口令密文长度异常 (必须为 48 字节 / 96 hex)".into());
        }
        if self.recovery_wrap.ciphertext_hex.len() != 96 {
            return Err("恢复密钥密文长度异常 (必须为 48 字节 / 96 hex)".into());
        }

        for field in [
            &self.password_salt_hex,
            &self.password_nonce_hex,
            &self.password_wrapped_key_hex,
            &self.recovery_wrap.salt_hex,
            &self.recovery_wrap.nonce_hex,
            &self.recovery_wrap.ciphertext_hex,
        ] {
            if field.len() % 2 != 0 || !field.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err("保险库信封含非法十六进制字段".into());
            }
        }

        if let Some(dpapi) = &self.dpapi_wrap {
            if dpapi.nonce_hex.len() % 2 != 0
                || !dpapi.nonce_hex.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err("DPAPI Nonce 十六进制格式异常".into());
            }
            if dpapi.ciphertext_hex.len() % 2 != 0
                || !dpapi.ciphertext_hex.bytes().all(|b| b.is_ascii_hexdigit())
                || dpapi.ciphertext_hex.len() > 32 * 1024
            {
                return Err("DPAPI 封装密文格式异常".into());
            }
            if dpapi.enabled && dpapi.ciphertext_hex.is_empty() {
                return Err("已启用的 DPAPI 封装缺少密文".into());
            }
        }

        Ok(())
    }

    /// Returns envelope path within given app data directory
    pub fn envelope_path(app_data_dir: &Path) -> PathBuf {
        app_data_dir.join(VAULT_ENVELOPE_FILE_NAME)
    }

    /// Atomically persists this envelope to disk
    pub fn save_atomic(&self, app_data_dir: &Path) -> Result<(), String> {
        self.validate_constraints()?;
        let path = Self::envelope_path(app_data_dir);

        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("序列化保险库信封失败: {}", e))?;

        super::atomic_file::write_atomic(&path, json.as_bytes())
            .map_err(|e| format!("替换保险库信封失败（原信封保留）: {}", e))?;

        Ok(())
    }

    /// Loads and validates envelope from a specific file path
    pub fn load_from_path(path: &Path) -> Result<Option<Self>, String> {
        if !path.exists() {
            return Ok(None);
        }

        let file = fs::File::open(path).map_err(|e| format!("读取保险库信封文件失败: {}", e))?;
        let mut content = String::new();
        file.take(MAX_ENVELOPE_BYTES + 1)
            .read_to_string(&mut content)
            .map_err(|e| format!("读取保险库信封文件失败: {}", e))?;
        if content.len() as u64 > MAX_ENVELOPE_BYTES {
            return Err("保险库信封文件超过大小限制".into());
        }

        let envelope: Self = serde_json::from_str(&content)
            .map_err(|e| format!("解析保险库信封 JSON 失败: {}", e))?;

        envelope.validate_constraints()?;
        Ok(Some(envelope))
    }

    /// Loads and validates envelope from disk
    pub fn load_from_dir(app_data_dir: &Path) -> Result<Option<Self>, String> {
        Self::load_from_path(&Self::envelope_path(app_data_dir))
    }
}

/// Derives a 256-bit key using Argon2id with bound parameters
pub fn derive_key_argon2id(
    secret: &[u8],
    salt: &[u8],
    params: &KdfParameters,
) -> Result<[u8; 32], String> {
    let p = Params::new(
        params.mem_cost_kb,
        params.time_cost,
        params.parallelism,
        Some(32),
    )
    .map_err(|e| format!("Argon2 params error: {}", e))?;

    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, p);
    let mut derived_key = [0u8; 32];
    argon2
        .hash_password_into(secret, salt, &mut derived_key)
        .map_err(|e| format!("Argon2 计算派生密钥失败: {}", e))?;

    Ok(derived_key)
}

fn build_aad(version: u32, kdf_algo: &str, purpose: &str) -> Vec<u8> {
    format!("browsory-vault-v{}:{}:{}", version, kdf_algo, purpose).into_bytes()
}

/// Generates a standardized 256-bit recovery key with canonical grouping:
/// BHA-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX
pub fn generate_recovery_key() -> (String, [u8; 32]) {
    let mut raw_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw_bytes);

    let hex_str = hex::encode(raw_bytes).to_uppercase();
    let mut chunks = vec!["BHA".to_string()];
    for i in (0..hex_str.len()).step_by(4) {
        chunks.push(hex_str[i..i + 4].to_string());
    }
    (chunks.join("-"), raw_bytes)
}

/// Normalizes a recovery key string (strips 'BHA', dashes, colons, spaces)
pub fn normalize_recovery_key(input: &str) -> Result<[u8; 32], String> {
    let upper = input.trim().to_uppercase();
    let stripped = upper.trim_start_matches("BHA").replace(['-', ' ', ':'], "");
    if stripped.len() != 64 {
        return Err(format!(
            "恢复密钥字符数量异常 (期望 64 位十六进制字符，实际 {})",
            stripped.len()
        ));
    }
    let bytes = hex::decode(&stripped).map_err(|_| "恢复密钥包含非十六进制字符".to_string())?;
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

/// Creates a new vault envelope wrapping a freshly generated 256-bit database master key.
pub fn create_vault_envelope(
    password: &str,
    kdf_params: Option<KdfParameters>,
    now_ms: i64,
) -> Result<(VaultEnvelope, String, MasterKey), String> {
    if password.trim().chars().count() < MIN_PASSWORD_LEN {
        return Err(format!("主加密口令长度不得少于 {} 位", MIN_PASSWORD_LEN));
    }

    // 1. Generate 256-bit database Master Key
    let master_key = MasterKey::generate();

    // 2. Generate Recovery Key
    let (recovery_key_str, recovery_key_bytes) = generate_recovery_key();
    let recovery_key_bytes = Zeroizing::new(recovery_key_bytes);

    let params = kdf_params.unwrap_or_default();

    // 3. Encrypt Master Key with Password KEK + AAD
    let mut pw_salt = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut pw_salt);
    let mut pw_kek = derive_key_argon2id(password.as_bytes(), &pw_salt, &params)?;

    let mut pw_nonce = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut pw_nonce);

    let pw_cipher =
        Aes256Gcm::new_from_slice(&pw_kek).map_err(|e| format!("Aes256Gcm 初始化错误: {}", e))?;
    pw_kek.zeroize();

    let pw_aad = build_aad(ENVELOPE_CURRENT_VERSION, KDF_ALGO_ARGON2ID, "password");
    let pw_ciphertext = pw_cipher
        .encrypt(
            Nonce::from_slice(&pw_nonce),
            Payload {
                msg: &master_key.0,
                aad: &pw_aad,
            },
        )
        .map_err(|e| format!("主密钥口令封装失败: {}", e))?;

    // 4. Encrypt Master Key with Recovery Key KEK + AAD
    let mut rec_salt = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut rec_salt);
    let mut rec_kek = derive_key_argon2id(recovery_key_bytes.as_ref(), &rec_salt, &params)?;

    let mut rec_nonce = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut rec_nonce);

    let rec_cipher =
        Aes256Gcm::new_from_slice(&rec_kek).map_err(|e| format!("Aes256Gcm 初始化错误: {}", e))?;
    rec_kek.zeroize();

    let rec_aad = build_aad(ENVELOPE_CURRENT_VERSION, KDF_ALGO_ARGON2ID, "recovery");
    let rec_ciphertext = rec_cipher
        .encrypt(
            Nonce::from_slice(&rec_nonce),
            Payload {
                msg: &master_key.0,
                aad: &rec_aad,
            },
        )
        .map_err(|e| format!("主密钥恢复密钥封装失败: {}", e))?;

    let envelope = VaultEnvelope {
        version: ENVELOPE_CURRENT_VERSION,
        kdf_algorithm: KDF_ALGO_ARGON2ID.to_string(),
        kdf_parameters: params,
        password_salt_hex: hex::encode(pw_salt),
        password_nonce_hex: hex::encode(pw_nonce),
        password_wrapped_key_hex: hex::encode(pw_ciphertext),
        recovery_wrap: RecoveryWrap {
            salt_hex: hex::encode(rec_salt),
            nonce_hex: hex::encode(rec_nonce),
            ciphertext_hex: hex::encode(rec_ciphertext),
        },
        dpapi_wrap: None,
        created_at: now_ms,
        updated_at: now_ms,
        recovery_key_confirmed: false,
    };

    Ok((envelope, recovery_key_str, master_key))
}

/// Replaces the recovery wrap after password authentication by the caller.
pub fn rotate_recovery_wrap(
    envelope: &mut VaultEnvelope,
    key: &MasterKey,
    now_ms: i64,
) -> Result<String, String> {
    envelope.validate_constraints()?;
    let (display, bytes) = generate_recovery_key();
    let bytes = Zeroizing::new(bytes);
    let mut salt = [0u8; 32];
    let mut nonce = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let kek = Zeroizing::new(derive_key_argon2id(
        bytes.as_ref(),
        &salt,
        &envelope.kdf_parameters,
    )?);
    let cipher = Aes256Gcm::new_from_slice(kek.as_ref()).map_err(|_| "恢复密钥封装初始化失败")?;
    let aad = build_aad(envelope.version, &envelope.kdf_algorithm, "recovery");
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: key.as_bytes(),
                aad: &aad,
            },
        )
        .map_err(|_| "恢复密钥封装失败")?;
    envelope.recovery_wrap = RecoveryWrap {
        salt_hex: hex::encode(salt),
        nonce_hex: hex::encode(nonce),
        ciphertext_hex: hex::encode(ciphertext),
    };
    envelope.recovery_key_confirmed = false;
    envelope.updated_at = now_ms;
    Ok(display)
}

/// Unwraps the Master Key from the envelope using the password.
pub fn unwrap_master_key_with_password(
    envelope: &VaultEnvelope,
    password: &str,
) -> Result<MasterKey, String> {
    envelope.validate_constraints()?;

    let salt = hex::decode(&envelope.password_salt_hex)
        .map_err(|_| "口令盐十六进制解码失败".to_string())?;
    let nonce = hex::decode(&envelope.password_nonce_hex)
        .map_err(|_| "口令 Nonce 十六进制解码失败".to_string())?;
    let ciphertext = hex::decode(&envelope.password_wrapped_key_hex)
        .map_err(|_| "口令密文十六进制解码失败".to_string())?;

    let mut kek = derive_key_argon2id(password.as_bytes(), &salt, &envelope.kdf_parameters)?;
    let cipher =
        Aes256Gcm::new_from_slice(&kek).map_err(|e| format!("Aes256Gcm 初始化错误: {}", e))?;
    kek.zeroize();

    let aad = build_aad(envelope.version, &envelope.kdf_algorithm, "password");
    let decrypted = Zeroizing::new(
        cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| "口令验证失败，无法解密保险库主密钥".to_string())?,
    );

    if decrypted.len() != 32 {
        return Err("解密出的主密钥长度异常 (必须为 32 字节)".to_string());
    }

    let mut key = [0u8; 32];
    key.copy_from_slice(&decrypted);
    Ok(MasterKey(key))
}

/// Unwraps the Master Key from the envelope using the Recovery Key.
pub fn unwrap_master_key_with_recovery(
    envelope: &VaultEnvelope,
    recovery_key_input: &str,
) -> Result<MasterKey, String> {
    envelope.validate_constraints()?;

    let rec_bytes = Zeroizing::new(normalize_recovery_key(recovery_key_input)?);
    let salt = hex::decode(&envelope.recovery_wrap.salt_hex)
        .map_err(|_| "恢复盐十六进制解码失败".to_string())?;
    let nonce = hex::decode(&envelope.recovery_wrap.nonce_hex)
        .map_err(|_| "恢复 Nonce 十六进制解码失败".to_string())?;
    let ciphertext = hex::decode(&envelope.recovery_wrap.ciphertext_hex)
        .map_err(|_| "恢复密文十六进制解码失败".to_string())?;

    let mut kek = derive_key_argon2id(rec_bytes.as_ref(), &salt, &envelope.kdf_parameters)?;
    let cipher =
        Aes256Gcm::new_from_slice(&kek).map_err(|e| format!("Aes256Gcm 初始化错误: {}", e))?;
    kek.zeroize();

    let aad = build_aad(envelope.version, &envelope.kdf_algorithm, "recovery");
    let decrypted = Zeroizing::new(
        cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| "恢复密钥无效或损坏，无法解密保险库主密钥".to_string())?,
    );

    if decrypted.len() != 32 {
        return Err("解密出的主密钥长度异常 (必须为 32 字节)".to_string());
    }

    let mut key = [0u8; 32];
    key.copy_from_slice(&decrypted);
    Ok(MasterKey(key))
}

/// Changes the password on an existing envelope by rewrapping the same Master Key.
/// Does NOT require re-encrypting the archive.db database file!
pub fn change_envelope_password(
    envelope: &mut VaultEnvelope,
    master_key: &MasterKey,
    new_password: &str,
    now_ms: i64,
) -> Result<(), String> {
    if new_password.trim().chars().count() < MIN_PASSWORD_LEN {
        return Err(format!("新口令长度不得少于 {} 位", MIN_PASSWORD_LEN));
    }

    let mut new_salt = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut new_salt);

    let mut kek =
        derive_key_argon2id(new_password.as_bytes(), &new_salt, &envelope.kdf_parameters)?;

    let mut new_nonce = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut new_nonce);

    let cipher =
        Aes256Gcm::new_from_slice(&kek).map_err(|e| format!("Aes256Gcm 初始化错误: {}", e))?;
    kek.zeroize();

    let aad = build_aad(envelope.version, &envelope.kdf_algorithm, "password");
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&new_nonce),
            Payload {
                msg: &master_key.0,
                aad: &aad,
            },
        )
        .map_err(|e| format!("重新封装口令失败: {}", e))?;

    envelope.password_salt_hex = hex::encode(new_salt);
    envelope.password_nonce_hex = hex::encode(new_nonce);
    envelope.password_wrapped_key_hex = hex::encode(ciphertext);
    envelope.updated_at = now_ms;

    Ok(())
}

impl VaultEnvelope {
    pub fn has_dpapi_wrap(&self) -> bool {
        self.dpapi_wrap.as_ref().is_some_and(|wrap| wrap.enabled)
    }
}

/// Wraps a MasterKey with Windows DPAPI bound to the current OS user account.
pub fn wrap_master_key_with_dpapi(master_key: &MasterKey) -> Result<DpapiWrap, String> {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = master_key;
        Err("DPAPI 自动密钥托管仅支持 Windows 平台".to_string())
    }
    #[cfg(target_os = "windows")]
    {
        let mut nonce = [0u8; 12];
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        let aad = build_aad(ENVELOPE_CURRENT_VERSION, KDF_ALGO_ARGON2ID, "dpapi");
        let mut payload = Zeroizing::new(Vec::with_capacity(aad.len() + nonce.len() + 32));
        payload.extend_from_slice(&aad);
        payload.extend_from_slice(&nonce);
        payload.extend_from_slice(&master_key.0);
        let protected = super::dpapi::dpapi_protect(&payload)
            .map_err(|e| format!("DPAPI 保护主密钥失败: {}", e))?;
        Ok(DpapiWrap {
            nonce_hex: hex::encode(nonce),
            ciphertext_hex: hex::encode(protected),
            enabled: true,
        })
    }
}

/// Unwraps the MasterKey from a Windows DPAPI-protected envelope.
pub fn unwrap_master_key_with_dpapi(envelope: &VaultEnvelope) -> Result<MasterKey, String> {
    envelope.validate_constraints()?;
    #[cfg(not(target_os = "windows"))]
    {
        let _ = envelope;
        Err("DPAPI 自动密钥托管仅支持 Windows 平台".to_string())
    }
    #[cfg(target_os = "windows")]
    {
        let wrap = envelope
            .dpapi_wrap
            .as_ref()
            .filter(|w| w.enabled)
            .ok_or_else(|| "当前保险库未启用本机 DPAPI 自动密钥托管".to_string())?;
        let expected_nonce =
            hex::decode(&wrap.nonce_hex).map_err(|_| "DPAPI Nonce 十六进制解码失败".to_string())?;
        if expected_nonce.len() != 12 {
            return Err("DPAPI Nonce 长度异常".to_string());
        }
        let ciphertext = hex::decode(&wrap.ciphertext_hex)
            .map_err(|_| "DPAPI 密文十六进制解码失败".to_string())?;
        let decrypted =
            Zeroizing::new(super::dpapi::dpapi_unprotect(&ciphertext).map_err(|_| {
                "本机 DPAPI 无法解密此数据库保险库（可能来自其他系统账户或设备）".to_string()
            })?);
        let aad = build_aad(envelope.version, &envelope.kdf_algorithm, "dpapi");
        let expected_len = aad.len() + expected_nonce.len() + 32;
        if decrypted.len() != expected_len
            || !decrypted.starts_with(&aad)
            || decrypted[aad.len()..aad.len() + expected_nonce.len()] != expected_nonce[..]
        {
            return Err("DPAPI 主密钥载荷校验失败".to_string());
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&decrypted[aad.len() + expected_nonce.len()..]);
        Ok(MasterKey(key))
    }
}

/// Creates a passive, DPAPI-managed envelope without a user-entered passphrase.
/// Recovery must be explicitly displayed and confirmed before this envelope is trusted.
pub fn create_auto_vault_envelope(now_ms: i64) -> Result<(VaultEnvelope, MasterKey), String> {
    let mut random_pw_bytes = Zeroizing::new([0u8; 24]);
    rand::rngs::OsRng.fill_bytes(random_pw_bytes.as_mut());
    let random_pw = Zeroizing::new(hex::encode(random_pw_bytes.as_ref()));
    let fast_params = KdfParameters {
        mem_cost_kb: MIN_MEM_COST_KB,
        time_cost: 1,
        parallelism: 1,
    };
    let (mut envelope, _recovery, master_key) =
        create_vault_envelope(random_pw.as_str(), Some(fast_params), now_ms)?;
    envelope.dpapi_wrap = Some(wrap_master_key_with_dpapi(&master_key)?);
    envelope.recovery_key_confirmed = false;
    envelope.updated_at = now_ms;
    Ok((envelope, master_key))
}

/// Enrolls DPAPI auto-unlock on an existing unlocked envelope if not already enabled.
pub fn enroll_dpapi_wrap(
    envelope: &mut VaultEnvelope,
    master_key: &MasterKey,
    now_ms: i64,
) -> Result<bool, String> {
    if envelope.has_dpapi_wrap() {
        return Ok(false);
    }
    envelope.dpapi_wrap = Some(wrap_master_key_with_dpapi(master_key)?);
    envelope.updated_at = now_ms;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vault_envelope_lifecycle_and_constraints() {
        let password = "Correct-Horse-Battery-Staple-99!";
        let now = 1710000000000;

        let (mut env, recovery_key, original_key) =
            create_vault_envelope(password, None, now).expect("Create envelope");

        // 1. Unwrap with correct password
        let unlocked = unwrap_master_key_with_password(&env, password).expect("Unwrap with pw");
        assert_eq!(original_key.0, unlocked.0);

        // 2. Reject wrong password
        assert!(unwrap_master_key_with_password(&env, "Wrong-Password-123").is_err());

        // 3. Unwrap with recovery key
        let recovered =
            unwrap_master_key_with_recovery(&env, &recovery_key).expect("Unwrap with recovery");
        assert_eq!(original_key.0, recovered.0);

        // 4. Change password
        let new_password = "Super-Strong-New-Master-Key-Password-2026";
        change_envelope_password(&mut env, &original_key, new_password, now + 5000)
            .expect("Change password");

        // Old password fails
        assert!(unwrap_master_key_with_password(&env, password).is_err());

        // New password unlocks original key
        let re_unlocked =
            unwrap_master_key_with_password(&env, new_password).expect("Unwrap with new pw");
        assert_eq!(original_key.0, re_unlocked.0);

        // Recovery key still unlocks original key after password change
        let rec_after =
            unwrap_master_key_with_recovery(&env, &recovery_key).expect("Recovery after pw change");
        assert_eq!(original_key.0, rec_after.0);

        // 5. Tampering with AAD metadata (e.g. version) causes GCM tag mismatch
        let mut tampered = env.clone();
        tampered.version = 2; // altered version
        assert!(unwrap_master_key_with_password(&tampered, new_password).is_err());

        // 6. Resource constraint validation (e.g. malicious mem_cost)
        let mut dos_env = env.clone();
        dos_env.kdf_parameters.mem_cost_kb = 1024 * 1024 * 10; // 10 GB attack
        assert!(dos_env.validate_constraints().is_err());
    }

    #[test]
    fn test_atomic_file_persistence_and_reload() {
        let temp_dir =
            std::env::temp_dir().join(format!("browsory_env_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();

        let (env, _, _) =
            create_vault_envelope("SecureVaultPassphrase#123", None, 1710000000000).unwrap();

        // Save
        env.save_atomic(&temp_dir).unwrap();
        assert!(VaultEnvelope::envelope_path(&temp_dir).exists());

        // Reload
        let loaded = VaultEnvelope::load_from_dir(&temp_dir)
            .unwrap()
            .expect("Loaded envelope");
        assert_eq!(env, loaded);
        let mut legacy_json = serde_json::to_value(&env).unwrap();
        legacy_json
            .as_object_mut()
            .unwrap()
            .remove("recovery_key_confirmed");
        let legacy: VaultEnvelope = serde_json::from_value(legacy_json).unwrap();
        assert!(
            legacy.recovery_key_confirmed,
            "older envelopes retain their previous confirmation behavior"
        );

        let mut updated = env.clone();
        updated.updated_at += 1;
        updated.save_atomic(&temp_dir).unwrap();
        assert_eq!(
            VaultEnvelope::load_from_dir(&temp_dir).unwrap().unwrap(),
            updated
        );
        // Invalid replacement must not disturb the existing envelope.
        let mut invalid = updated.clone();
        invalid.password_nonce_hex = "g".repeat(24);
        assert!(invalid.save_atomic(&temp_dir).is_err());
        assert_eq!(
            VaultEnvelope::load_from_dir(&temp_dir).unwrap().unwrap(),
            updated
        );
        fs::write(
            VaultEnvelope::envelope_path(&temp_dir),
            vec![b' '; MAX_ENVELOPE_BYTES as usize + 1],
        )
        .unwrap();
        assert!(VaultEnvelope::load_from_dir(&temp_dir).is_err());

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn short_or_multibyte_short_password_is_rejected_before_kdf() {
        assert!(create_vault_envelope("123456", None, 0).is_err());
        assert!(create_vault_envelope("密码太短", None, 0).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn failed_envelope_replacement_keeps_old_credentials() {
        use std::os::windows::fs::OpenOptionsExt;
        let root =
            std::env::temp_dir().join(format!("browsory_envelope_locked_{}", uuid::Uuid::new_v4()));
        let params = KdfParameters {
            mem_cost_kb: MIN_MEM_COST_KB,
            time_cost: 1,
            parallelism: 1,
        };
        let (mut envelope, _, _) =
            create_vault_envelope("Original-password-test", Some(params), 0).unwrap();
        envelope.save_atomic(&root).unwrap();
        let old = fs::read(VaultEnvelope::envelope_path(&root)).unwrap();
        let lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(VaultEnvelope::envelope_path(&root))
            .unwrap();
        envelope.updated_at = 1;
        assert!(envelope.save_atomic(&root).is_err());
        drop(lock);
        assert_eq!(fs::read(VaultEnvelope::envelope_path(&root)).unwrap(), old);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn passive_dpapi_envelope_roundtrips_and_rejects_tampering() {
        let (env, original_key) = create_auto_vault_envelope(1710000000000).unwrap();
        assert!(env.has_dpapi_wrap());
        assert!(!env.recovery_key_confirmed);
        let unwrapped = unwrap_master_key_with_dpapi(&env).unwrap();
        assert_eq!(original_key.0, unwrapped.0);

        let mut tampered_nonce = env.clone();
        tampered_nonce.dpapi_wrap.as_mut().unwrap().nonce_hex = "00".repeat(12);
        if tampered_nonce.dpapi_wrap != env.dpapi_wrap {
            assert!(unwrap_master_key_with_dpapi(&tampered_nonce).is_err());
        }

        let mut disabled = env.clone();
        disabled.dpapi_wrap.as_mut().unwrap().enabled = false;
        assert!(!disabled.has_dpapi_wrap());
        assert!(unwrap_master_key_with_dpapi(&disabled).is_err());

        let (mut legacy_env, _, legacy_key) = create_vault_envelope(
            "Legacy-passphrase-2026",
            Some(KdfParameters {
                mem_cost_kb: MIN_MEM_COST_KB,
                time_cost: 1,
                parallelism: 1,
            }),
            0,
        )
        .unwrap();
        assert!(!legacy_env.has_dpapi_wrap());
        assert!(enroll_dpapi_wrap(&mut legacy_env, &legacy_key, 100).unwrap());
        assert!(!enroll_dpapi_wrap(&mut legacy_env, &legacy_key, 200).unwrap());
        assert_eq!(
            unwrap_master_key_with_dpapi(&legacy_env).unwrap().0,
            legacy_key.0
        );
    }
}
