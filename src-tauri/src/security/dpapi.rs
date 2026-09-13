#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::LocalFree;
#[cfg(target_os = "windows")]
use windows_sys::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
};

/// Encrypts data using Windows DPAPI (tied to current Windows user credentials).
/// Returns unmodified data on non-Windows platforms.
pub fn dpapi_protect(data: &[u8]) -> Result<Vec<u8>, String> {
    #[cfg(target_os = "windows")]
    {
        if data.is_empty() {
            return Ok(Vec::new());
        }

        unsafe {
            let in_blob = CRYPT_INTEGER_BLOB {
                cbData: data.len() as u32,
                pbData: data.as_ptr() as *mut u8,
            };
            let mut out_blob = CRYPT_INTEGER_BLOB {
                cbData: 0,
                pbData: std::ptr::null_mut(),
            };

            let success = CryptProtectData(
                &in_blob,
                std::ptr::null(),     // no description
                std::ptr::null_mut(), // no extra entropy
                std::ptr::null_mut(), // reserved
                std::ptr::null_mut(), // no prompt
                0x1,                  // CRYPTPROTECT_UI_FORBIDDEN
                &mut out_blob,
            );

            if success == 0 {
                return Err("DPAPI CryptProtectData failed".to_string());
            }

            let result =
                std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec();
            LocalFree(out_blob.pbData as _);
            Ok(result)
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        Ok(data.to_vec())
    }
}

/// Decrypts data using Windows DPAPI.
pub fn dpapi_unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
    #[cfg(target_os = "windows")]
    {
        if data.is_empty() {
            return Ok(Vec::new());
        }

        unsafe {
            let in_blob = CRYPT_INTEGER_BLOB {
                cbData: data.len() as u32,
                pbData: data.as_ptr() as *mut u8,
            };
            let mut out_blob = CRYPT_INTEGER_BLOB {
                cbData: 0,
                pbData: std::ptr::null_mut(),
            };

            let success = CryptUnprotectData(
                &in_blob,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0x1,
                &mut out_blob,
            );

            if success == 0 {
                return Err("DPAPI CryptUnprotectData failed".to_string());
            }

            let result =
                std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec();
            LocalFree(out_blob.pbData as _);
            Ok(result)
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        Ok(data.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dpapi_roundtrip() {
        let plaintext = b"Browsory Secure Master Key 2026";
        let protected = dpapi_protect(plaintext).expect("protect should succeed");
        let unprotected = dpapi_unprotect(&protected).expect("unprotect should succeed");
        assert_eq!(plaintext.to_vec(), unprotected);
    }
}
