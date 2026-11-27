use keyring::Entry;
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{error, info, warn};

use super::atomic_file::write_atomic;
use super::dpapi::{dpapi_protect, dpapi_unprotect};

const SERVICE_NAME: &str = "browsory";
const NAMESPACE_VERSION: &str = "v1";

/// Saves a secret in the namespaced native store, with an encrypted file fallback.
///
/// The native write is accepted only after reading it through a newly-created
/// `Entry`. This matters on Windows, where a successful call on an invalid or
/// unavailable backend must not cause the existing fallback to be removed.
pub fn set_secret(key: &str, value: &str, app_dir: &Path) -> Result<(), String> {
    validate_key(key)?;
    let trimmed = value.trim();

    if trimmed.is_empty() {
        return clear_secret(key, app_dir);
    }

    // Commit a verified, directory-scoped encrypted copy before changing the
    // native entry. If the native store later becomes readable with an older
    // value, this fallback remains authoritative until a verified native write.
    #[cfg(target_os = "windows")]
    {
        save_secret_fallback(key, trimmed, app_dir)?;
        if load_secret_fallback(key, app_dir)?.as_deref() != Some(trimmed) {
            return Err("DPAPI secret read-back did not match the value written".into());
        }
    }
    match write_native_and_verify(key, trimmed, app_dir) {
        Ok(()) => cleanup_fallbacks(key, app_dir),
        Err(native_error) => {
            warn!(
                "Native secret store unavailable for key '{}', using DPAPI fallback: {}",
                key, native_error
            );
            save_secret_fallback(key, trimmed, app_dir).map_err(|fallback_error| {
                format!(
                    "Native secret store failed ({}); DPAPI fallback failed ({})",
                    native_error, fallback_error
                )
            })
        }
    }
}

/// Retrieves a secret from the namespaced native store or encrypted fallback.
pub fn get_secret(key: &str, app_dir: &Path) -> Result<Option<String>, String> {
    validate_key(key)?;

    if let Some(password) = load_secret_fallback(key, app_dir)? {
        return Ok(Some(password));
    }

    match namespaced_entry(key, app_dir) {
        Ok(entry) => match entry.get_password() {
            Ok(password) => return Ok(Some(password)),
            Err(keyring::Error::NoEntry) => {}
            Err(error) => warn!(
                "Native secret read failed for key '{}', checking fallback: {}",
                key, error
            ),
        },
        Err(error) => warn!(
            "Native secret entry unavailable for key '{}', checking fallback: {}",
            key, error
        ),
    }

    // The pre-namespace service is considered only for the standard installed
    // data directory. Portable directories must never share it with another
    // installation. A successful read is explicitly migrated to the new entry;
    // the old entry is retained if migration fails.
    if let Some(legacy_entry) = legacy_entry(key, app_dir) {
        match legacy_entry.get_password() {
            Ok(password) => {
                if let Err(error) = migrate_legacy_native(key, &password, app_dir, &legacy_entry) {
                    warn!(
                        "Legacy native secret migration failed for key '{}': {}",
                        key, error
                    );
                }
                return Ok(Some(password));
            }
            Err(keyring::Error::NoEntry) => {}
            Err(error) => warn!(
                "Legacy native secret read failed for key '{}': {}",
                key, error
            ),
        }
    }

    // Fallback files written by versions before namespacing are already
    // scoped to app_dir, so they are safe to consider for every installation.
    if let Some(password) = load_legacy_fallback(key, app_dir)? {
        if let Err(error) = save_secret_fallback(key, &password, app_dir) {
            warn!(
                "Legacy fallback migration failed for key '{}': {}",
                key, error
            );
        } else if let Err(error) = remove_legacy_fallback(key, app_dir) {
            // Keep the migrated value. Failure to remove old data is visible,
            // but must not turn a successful read into data loss.
            warn!(
                "Legacy fallback cleanup failed for key '{}': {}",
                key, error
            );
        }
        return Ok(Some(password));
    }

    Ok(None)
}

/// Deletes a secret from the namespaced native store, legacy installed entry,
/// and both fallback paths. Deletion errors are returned instead of ignored.
pub fn delete_secret(key: &str, app_dir: &Path) -> Result<(), String> {
    validate_key(key)?;
    clear_secret(key, app_dir)
}

fn clear_secret(key: &str, app_dir: &Path) -> Result<(), String> {
    let mut errors = Vec::new();

    match namespaced_entry(key, app_dir) {
        Ok(entry) => match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(error) => errors.push(format!("namespaced native delete failed: {}", error)),
        },
        Err(error) => errors.push(format!("namespaced native entry failed: {}", error)),
    }

    if let Some(entry) = legacy_entry(key, app_dir) {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(error) => errors.push(format!("legacy native delete failed: {}", error)),
        }
    }

    for result in [
        remove_secret_fallback(key, app_dir),
        remove_legacy_fallback(key, app_dir),
    ] {
        if let Err(error) = result {
            errors.push(error);
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn validate_key(key: &str) -> Result<(), String> {
    if key.is_empty() || key.len() > 128 {
        return Err("Secret key must contain 1-128 characters".to_string());
    }
    if !key
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err("Secret key contains unsupported path characters".to_string());
    }
    Ok(())
}

fn namespaced_entry(key: &str, app_dir: &Path) -> Result<Entry, String> {
    let user = format!("{}:{}", app_namespace(app_dir), key);
    Entry::new(SERVICE_NAME, &user)
        .map_err(|error| format!("failed to create namespaced native entry: {}", error))
}

fn write_native_and_verify(key: &str, value: &str, app_dir: &Path) -> Result<(), String> {
    let entry = namespaced_entry(key, app_dir)?;
    entry
        .set_password(value)
        .map_err(|error| format!("native secret write failed: {}", error))?;

    // Deliberately create an independent Entry to verify persistence through
    // the backend, rather than trusting the object used for the write.
    let verification_entry = namespaced_entry(key, app_dir)?;
    let persisted = verification_entry
        .get_password()
        .map_err(|error| format!("native secret read-back failed: {}", error))?;
    if persisted != value {
        return Err("native secret read-back did not match the value written".to_string());
    }
    Ok(())
}

fn migrate_legacy_native(
    key: &str,
    value: &str,
    app_dir: &Path,
    legacy_entry: &Entry,
) -> Result<(), String> {
    write_native_and_verify(key, value, app_dir)?;
    match legacy_entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(format!("legacy native cleanup failed: {}", error)),
    }
}

fn app_namespace(app_dir: &Path) -> String {
    let normalized = normalize_path(app_dir);
    let digest = Sha256::digest(normalized.as_bytes());
    format!("{}-{}", NAMESPACE_VERSION, hex::encode(&digest[..16]))
}

fn normalize_path(path: &Path) -> String {
    let absolute = fs::canonicalize(path).unwrap_or_else(|_| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(path)
        }
    });
    let mut normalized = absolute.to_string_lossy().replace('\\', "/").to_lowercase();
    while normalized.len() > 1 && normalized.ends_with('/') {
        normalized.pop();
    }
    normalized
}

fn is_portable_app_dir(app_dir: &Path) -> bool {
    app_dir.parent().is_some_and(|exe_dir| {
        exe_dir.join("portable.txt").is_file() || exe_dir.join("portable").is_file()
    })
}

fn is_standard_install_dir(app_dir: &Path) -> bool {
    if is_portable_app_dir(app_dir) {
        return false;
    }
    directories::ProjectDirs::from("com", "browsory", "app")
        .is_some_and(|dirs| normalize_path(app_dir) == normalize_path(dirs.data_local_dir()))
}

fn legacy_entry(key: &str, app_dir: &Path) -> Option<Entry> {
    if !is_standard_install_dir(app_dir) {
        return None;
    }
    Entry::new(SERVICE_NAME, key).ok()
}

fn fallback_path(key: &str, app_dir: &Path) -> PathBuf {
    app_dir
        .join("secrets")
        .join(app_namespace(app_dir))
        .join(format!("{}.enc", key))
}

fn legacy_fallback_path(key: &str, app_dir: &Path) -> PathBuf {
    app_dir.join("secrets").join(format!("{}.enc", key))
}

fn save_secret_fallback(key: &str, value: &str, app_dir: &Path) -> Result<(), String> {
    if value.is_empty() {
        return remove_secret_fallback(key, app_dir);
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (key, app_dir);
        return Err("DPAPI fallback is unavailable on non-Windows platforms".to_string());
    }

    #[cfg(target_os = "windows")]
    {
        let path = fallback_path(key, app_dir);
        let encrypted = dpapi_protect(value.as_bytes())
            .map_err(|error| format!("DPAPI fallback encryption failed: {}", error))?;
        write_atomic(&path, &encrypted)
            .map_err(|error| format!("Failed to atomically write fallback secret: {}", error))?;
        Ok(())
    }
}

fn load_secret_fallback(key: &str, app_dir: &Path) -> Result<Option<String>, String> {
    load_fallback_at(&fallback_path(key, app_dir))
}

fn load_legacy_fallback(key: &str, app_dir: &Path) -> Result<Option<String>, String> {
    load_fallback_at(&legacy_fallback_path(key, app_dir))
}

fn load_fallback_at(path: &Path) -> Result<Option<String>, String> {
    if !path.exists() {
        return Ok(None);
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        return Err("Refusing to read a plaintext fallback on non-Windows platforms".to_string());
    }

    #[cfg(target_os = "windows")]
    {
        let raw = fs::read(path)
            .map_err(|error| format!("Failed to read fallback secret file: {}", error))?;
        let decrypted = dpapi_unprotect(&raw)
            .map_err(|error| format!("DPAPI fallback decryption failed: {}", error))?;
        let text = String::from_utf8(decrypted)
            .map_err(|error| format!("Secret utf8 decode error: {}", error))?;
        Ok(Some(text))
    }
}

fn cleanup_fallbacks(key: &str, app_dir: &Path) -> Result<(), String> {
    let mut errors = Vec::new();
    for result in [
        remove_secret_fallback(key, app_dir),
        remove_legacy_fallback(key, app_dir),
    ] {
        if let Err(error) = result {
            errors.push(error);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn remove_secret_fallback(key: &str, app_dir: &Path) -> Result<(), String> {
    remove_fallback_at(&fallback_path(key, app_dir))
}

fn remove_legacy_fallback(key: &str, app_dir: &Path) -> Result<(), String> {
    remove_fallback_at(&legacy_fallback_path(key, app_dir))
}

fn remove_fallback_at(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Failed to remove fallback secret file: {}", error)),
    }
}

/// One-time migration: migrates plain text ai_key from settings table into secure store
pub fn migrate_plain_secrets_from_db(conn: &Connection, app_dir: &Path) {
    let query_res: Result<String, _> = conn.query_row(
        "SELECT value FROM settings WHERE key = 'ai_key' LIMIT 1",
        [],
        |row| row.get(0),
    );

    if let Ok(plain_key) = query_res {
        let trimmed = plain_key.trim();
        if !trimmed.is_empty() {
            info!("Migrating existing plain-text ai_key from SQLite into OS Secret Store...");
            if let Err(error) = set_secret("ai_key", trimmed, app_dir) {
                error!("Failed to store secret into OS Secret Store: {}", error);
                return;
            }
        }
        // Permanently wipe plain key from SQLite database
        let _ = conn.execute("DELETE FROM settings WHERE key = 'ai_key'", []);
        info!("Successfully erased plain-text ai_key from settings table.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_root(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("browsory_secret_{}_{}", name, uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn app_directory_namespaces_are_normalized_and_isolated() {
        let root = test_root("namespace");
        let first = root.join("first");
        let second = root.join("second");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();

        assert_ne!(app_namespace(&first), app_namespace(&second));
        assert_eq!(app_namespace(&first), app_namespace(&first.join(".")));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn portable_data_directory_never_uses_legacy_service() {
        let root = test_root("portable");
        fs::write(root.join("portable.txt"), b"portable").unwrap();
        let data = root.join("data");
        fs::create_dir_all(&data).unwrap();

        assert!(is_portable_app_dir(&data));
        assert!(!is_standard_install_dir(&data));
        assert!(legacy_entry("test_key", &data).is_none());

        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_dpapi_fallback_roundtrip_and_reopen() {
        let root = test_root("dpapi");
        let app_dir = root.join("data");
        fs::create_dir_all(&app_dir).unwrap();

        save_secret_fallback("test_key", "secret-value", &app_dir).unwrap();
        let reopened = load_secret_fallback("test_key", &app_dir).unwrap();
        assert_eq!(reopened.as_deref(), Some("secret-value"));
        assert!(fallback_path("test_key", &app_dir).starts_with(&app_dir));

        remove_secret_fallback("test_key", &app_dir).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_independent_entry_survives_reopen_and_directory_isolation() {
        let root = test_root("native");
        let first = root.join("first");
        let second = root.join("second");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        let key = format!("test_key_{}", uuid::Uuid::new_v4().simple());

        set_secret(&key, "native-value", &first).unwrap();
        let reopened = namespaced_entry(&key, &first).unwrap();
        assert_eq!(reopened.get_password().unwrap(), "native-value");
        assert_eq!(get_secret(&key, &second).unwrap(), None);

        delete_secret(&key, &first).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn newer_fallback_is_not_shadowed_by_an_old_native_entry() {
        let root = test_root("fallback_precedence");
        write_native_and_verify("test_key", "old-native", &root).unwrap();
        save_secret_fallback("test_key", "new-fallback", &root).unwrap();
        assert_eq!(
            get_secret("test_key", &root).unwrap().as_deref(),
            Some("new-fallback")
        );
        set_secret("test_key", "verified-new-native", &root).unwrap();
        assert!(!fallback_path("test_key", &root).exists());
        assert_eq!(
            get_secret("test_key", &root).unwrap().as_deref(),
            Some("verified-new-native")
        );
        delete_secret("test_key", &root).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn failed_fallback_replacement_preserves_old_native_and_encrypted_copy() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = test_root("failed_replace");
        write_native_and_verify("test_key", "old-value", &root).unwrap();
        save_secret_fallback("test_key", "old-value", &root).unwrap();
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(fallback_path("test_key", &root))
            .unwrap();
        assert!(set_secret("test_key", "replacement", &root).is_err());
        assert_eq!(
            namespaced_entry("test_key", &root)
                .unwrap()
                .get_password()
                .unwrap(),
            "old-value"
        );
        assert_eq!(
            load_secret_fallback("test_key", &root).unwrap().as_deref(),
            Some("old-value")
        );
        drop(held);
        delete_secret("test_key", &root).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn subprocess_probe() {
        let Some(directory) = std::env::var_os("BROWSORY_SECRET_TEST_DIRECTORY") else {
            return;
        };
        let value = get_secret("cross_process_fixture", Path::new(&directory)).unwrap();
        assert_eq!(value.as_deref(), Some("SYNTHETIC-CROSS-PROCESS-VALUE"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn saved_secret_is_readable_in_a_fresh_process() {
        let root = test_root("fresh_process");
        set_secret(
            "cross_process_fixture",
            "SYNTHETIC-CROSS-PROCESS-VALUE",
            &root,
        )
        .unwrap();
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "security::secret_store::tests::subprocess_probe"])
            .env("BROWSORY_SECRET_TEST_DIRECTORY", &root)
            .output()
            .unwrap();
        assert!(result.status.success(), "fresh-process secret read failed");
        delete_secret("cross_process_fixture", &root).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn non_windows_never_writes_plaintext_fallback() {
        let root = test_root("non_windows");
        let app_dir = root.join("data");
        fs::create_dir_all(&app_dir).unwrap();
        assert!(save_secret_fallback("test_key", "secret-value", &app_dir).is_err());
        assert!(!fallback_path("test_key", &app_dir).exists());
        fs::remove_dir_all(root).unwrap();
    }
}
