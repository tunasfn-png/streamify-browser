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

    if let Some(channel) = trimmed.strip_prefix("twitch:") {
        if !channel.is_empty()
            && channel.len() <= 25
            && channel
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return format!(
                "https://www.twitch.tv/popout/{}/chat",
                channel.to_ascii_lowercase()
            );
        }
    }

    // Already has a scheme
    if is_http(trimmed) {
        return trimmed.to_owned();
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
    if let Some(rest) = trimmed.strip_prefix("localhost") {
        if rest.is_empty() || rest.starts_with(':') || rest.starts_with('/') {
            let url = format!("http://{trimmed}");
            if is_http(&url) {
                return url;
            }
        }
    }

    // Check if it looks like a domain: has a dot, no spaces, reasonably formatted
    if !trimmed.contains(char::is_whitespace) && trimmed.contains('.') {
        // Basic domain-like check: at least one dot with text on both sides
        let host_part = trimmed.split('/').next().unwrap_or(trimmed);
        if let Some(dot_pos) = host_part.rfind('.') {
            let after_dot = &host_part[dot_pos + 1..];
            let before_dot = &host_part[..dot_pos];
            // TLD must be at least 2 chars, host part must have content
            if after_dot.len() >= 2 && !before_dot.is_empty() {
                // Strip optional port for validation
                let tld = after_dot.split(':').next().unwrap_or(after_dot);
                if tld.chars().all(|c| c.is_ascii_alphanumeric()) {
                    let url = format!("https://{trimmed}");
                    if is_http(&url) {
                        return url;
                    }
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
    if !url
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("http://"))
        && !url
            .get(..8)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("https://"))
    {
        return false;
    }
    url::Url::parse(url).is_ok_and(|parsed| parsed.has_host())
}

pub fn is_navigable(url: &str) -> bool {
    url == "about:blank" || is_http(url)
}

pub fn is_spotify(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|parsed| {
        parsed.scheme() == "https" && parsed.host_str() == Some("open.spotify.com")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_full_url_passthrough() {
        assert_eq!(normalize_url("https://example.com"), "https://example.com");
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
    fn expands_twitch_chat_and_validates_web_navigation() {
        assert_eq!(
            normalize_url("twitch:Some_Channel"),
            "https://www.twitch.tv/popout/some_channel/chat"
        );
        assert!(is_navigable("HTTPS://example.org/"));
        assert!(!is_navigable("file:///private"));
        assert!(!is_spotify("https://open.spotify.com.example.org/"));
        assert!(is_spotify("https://open.spotify.com/collection/tracks"));
    }
}
