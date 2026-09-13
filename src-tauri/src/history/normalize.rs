use url::Url;

pub struct NormalizedUrl {
    pub raw_url: String,
    pub normalized_url: String,
    pub domain: String,
}

pub fn normalize_url(raw_url: &str) -> NormalizedUrl {
    let raw = raw_url.trim();

    if let Ok(mut parsed) = Url::parse(raw) {
        let domain = parsed.host_str().unwrap_or("").to_lowercase();

        // 1. Lowercase scheme
        let scheme = parsed.scheme().to_lowercase();
        let _ = parsed.set_scheme(&scheme);

        // 2. Remove fragment (#...)
        parsed.set_fragment(None);

        // 3. Default ports (80 for http, 443 for https) are automatically stripped by Url::parse

        let mut norm = parsed.to_string();

        // 4. Safe trailing slash trim if path is just "/"
        if norm.ends_with('/') && parsed.path() == "/" && parsed.query().is_none() {
            norm.pop();
        }

        NormalizedUrl {
            raw_url: raw.to_string(),
            normalized_url: norm,
            domain,
        }
    } else {
        // Fallback for non-standard or malformed URLs
        NormalizedUrl {
            raw_url: raw.to_string(),
            normalized_url: raw.to_string(),
            domain: String::new(),
        }
    }
}
