use regex::Regex;
use std::sync::OnceLock;

static EMAIL_REGEX: OnceLock<Regex> = OnceLock::new();
static URL_PARAM_REGEX: OnceLock<Regex> = OnceLock::new();
static BEARER_REGEX: OnceLock<Regex> = OnceLock::new();
static CHINESE_ID_REGEX: OnceLock<Regex> = OnceLock::new();
static PHONE_REGEX: OnceLock<Regex> = OnceLock::new();

fn get_email_regex() -> &'static Regex {
    EMAIL_REGEX.get_or_init(|| {
        Regex::new(r"(?i)\b[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}\b").expect("Valid regex")
    })
}

fn get_url_param_regex() -> &'static Regex {
    URL_PARAM_REGEX.get_or_init(|| {
        Regex::new(r"(?i)([?&](?:token|access_token|secret|password|passwd|auth|api_key|key|session|code|sign)=)[^&\s#]+").expect("Valid regex")
    })
}

fn get_bearer_regex() -> &'static Regex {
    BEARER_REGEX
        .get_or_init(|| Regex::new(r"(?i)(bearer\s+)[a-zA-Z0-9_\-\.]{20,}").expect("Valid regex"))
}

fn get_chinese_id_regex() -> &'static Regex {
    CHINESE_ID_REGEX.get_or_init(|| {
        Regex::new(
            r"\b[1-9]\d{5}(?:18|19|20)\d{2}(?:0[1-9]|1[0-2])(?:0[1-9]|[12]\d|3[01])\d{3}[\dXx]\b",
        )
        .expect("Valid regex")
    })
}

fn get_phone_regex() -> &'static Regex {
    PHONE_REGEX.get_or_init(|| Regex::new(r"\b1[3-9]\d{9}\b").expect("Valid regex"))
}

/// Sanitizes prompts before dispatching to external (cloud) LLMs.
/// When `is_local` is true, data remains on the user's localhost (e.g. Ollama),
/// so aggressive masking can be relaxed.
pub fn sanitize_prompt_for_ai(input: &str, is_local: bool) -> String {
    if is_local {
        return input.to_string();
    }

    let mut sanitized = input.to_string();

    // 1. Redact URL sensitive parameters (tokens, passwords, session ids)
    sanitized = get_url_param_regex()
        .replace_all(&sanitized, "${1}[REDACTED]")
        .to_string();

    // 2. Redact Bearer authorization headers/tokens
    sanitized = get_bearer_regex()
        .replace_all(&sanitized, "${1}[TOKEN_REDACTED]")
        .to_string();

    // 3. Redact Email addresses
    sanitized = get_email_regex()
        .replace_all(&sanitized, "[EMAIL_REDACTED]")
        .to_string();

    // 4. Redact Chinese National ID numbers
    sanitized = get_chinese_id_regex()
        .replace_all(&sanitized, "[ID_REDACTED]")
        .to_string();

    // 5. Redact Chinese Mobile Phone numbers
    sanitized = get_phone_regex()
        .replace_all(&sanitized, "[PHONE_REDACTED]")
        .to_string();

    sanitized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_cloud_prompt() {
        let raw = "用户在问 https://example.com/api?token=secret123456&user=alice，联系邮箱 test@example.com，手机 13812345678。";
        let cleaned = sanitize_prompt_for_ai(raw, false);

        assert!(!cleaned.contains("secret123456"));
        assert!(!cleaned.contains("test@example.com"));
        assert!(!cleaned.contains("13812345678"));
        assert!(cleaned.contains("[REDACTED]"));
        assert!(cleaned.contains("[EMAIL_REDACTED]"));
        assert!(cleaned.contains("[PHONE_REDACTED]"));
    }

    #[test]
    fn test_sanitize_local_prompt_skips_redaction() {
        let raw = "联系邮箱 test@example.com";
        let cleaned = sanitize_prompt_for_ai(raw, true);
        assert_eq!(cleaned, raw);
    }
}
