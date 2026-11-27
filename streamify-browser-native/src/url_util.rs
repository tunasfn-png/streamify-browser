// Streamify Browser v0.2.0 – URL normalization
// Ported from v0.1.3 commands.rs normalize_url logic.
// Only allows HTTP and HTTPS navigation. Detects bare domains vs search queries.

use crate::constants::SEARCH_URL;

/// Normalizes user input into a navigable URL.
/// - If it already starts with http:// or https://, returns as-is (validated).
/// - If it looks like a domain (contains a dot, no spaces), prepends https://
/// - If it's localhost or localhost:PORT, prepends http://
/// - Otherwise, treats as a search query and returns a DuckDuckGo search URL.
pub fn normalize_url(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // Already has a scheme
    if trimmed.starts_with("https://") || trimmed.starts_with("http://") {
        // Validate it parses as a URL
        if url::Url::parse(trimmed).is_ok() {
            return trimmed.to_string();
        }
        // If it doesn't parse, fall through to search
    }

    // about:blank is allowed
    if trimmed == "about:blank" {
        return trimmed.to_string();
    }

    // Block any other schemes (file://, javascript:, data:, ftp:, etc.)
    if trimmed.contains("://") {
        return format!("{}{}", SEARCH_URL, urlencoding::encode(trimmed));
    }

    // localhost with optional port
    if trimmed.starts_with("localhost") {
        let rest = &trimmed["localhost".len()..];
        if rest.is_empty() || rest.starts_with(':') || rest.starts_with('/') {
            return format!("http://{}", trimmed);
        }
    }

    // Check if it looks like a domain: has a dot, no spaces, reasonably formatted
    if !trimmed.contains(' ') && trimmed.contains('.') {
        // Basic domain-like check: at least one dot with text on both sides
        let parts: Vec<&str> = trimmed.splitn(2, '/').collect();
        let host_part = parts[0];
        if let Some(dot_pos) = host_part.rfind('.') {
            let after_dot = &host_part[dot_pos + 1..];
            let before_dot = &host_part[..dot_pos];
            // TLD must be at least 2 chars, host part must have content
            if after_dot.len() >= 2 && !before_dot.is_empty() {
                // Strip optional port for validation
                let tld = after_dot.split(':').next().unwrap_or(after_dot);
                if tld.chars().all(|c| c.is_ascii_alphanumeric()) {
                    return format!("https://{}", trimmed);
                }
            }
        }
    }

    // Everything else is a search query
    format!("{}{}", SEARCH_URL, urlencoding::encode(trimmed))
}

/// Returns true if the URL is the internal blank page.
pub fn is_blank(url: &str) -> bool {
    url == "about:blank" || url.is_empty()
}

/// Returns true if the URL scheme is HTTP or HTTPS.
pub fn is_http(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

/// Extracts a display-friendly version of the URL.
/// Removes "https://" prefix and trailing "/" for display.
pub fn display_url(url: &str) -> String {
    if url == "about:blank" {
        return String::new();
    }
    let s = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    s.strip_suffix('/').unwrap_or(s).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_full_url_passthrough() {
        assert_eq!(
            normalize_url("https://example.com"),
            "https://example.com"
        );
        assert_eq!(
            normalize_url("http://localhost:3000"),
            "http://localhost:3000"
        );
    }

    #[test]
    fn test_domain_detection() {
        assert_eq!(normalize_url("google.com"), "https://google.com");
        assert_eq!(
            normalize_url("docs.rs/webview2-com"),
            "https://docs.rs/webview2-com"
        );
    }

    #[test]
    fn test_localhost() {
        assert_eq!(normalize_url("localhost"), "http://localhost");
        assert_eq!(normalize_url("localhost:8080"), "http://localhost:8080");
        assert_eq!(
            normalize_url("localhost:8080/api"),
            "http://localhost:8080/api"
        );
    }

    #[test]
    fn test_search_query() {
        let result = normalize_url("rust webview2 tutorial");
        assert!(result.starts_with(SEARCH_URL));
        assert!(result.contains("rust"));
    }

    #[test]
    fn test_blocked_scheme() {
        let result = normalize_url("file:///etc/passwd");
        assert!(result.starts_with(SEARCH_URL));
        let result = normalize_url("javascript:alert(1)");
        assert!(result.starts_with(SEARCH_URL));
    }

    #[test]
    fn test_blank() {
        assert!(is_blank("about:blank"));
        assert!(is_blank(""));
        assert!(!is_blank("https://example.com"));
    }

    #[test]
    fn test_display_url() {
        assert_eq!(display_url("https://example.com/"), "example.com");
        assert_eq!(display_url("about:blank"), "");
    }
}
