//! URL normalization and sanitization utilities.
//!
//! Ensures consistent URL representation by normalizing hostnames, removing
//! fragments, and handling default ports.

use std::net::{Ipv4Addr, Ipv6Addr};

use url::{Host, Url};

/// Errors that can occur during URL normalization.
#[derive(Debug, thiserror::Error)]
pub enum UrlNormalizationError {
    #[error("Invalid URL format: {0}")]
    InvalidFormat(String),

    #[error("Only HTTP and HTTPS protocols are allowed")]
    UnsupportedProtocol,

    #[error("Failed to normalize URL: {0}")]
    NormalizationFailed(String),
}

/// Normalizes a URL to a canonical form.
///
/// # Normalization Rules
///
/// 1. **Protocol**: Only HTTP and HTTPS are allowed
/// 2. **Hostname**: Converted to lowercase
/// 3. **Default ports**: Removed (80 for HTTP, 443 for HTTPS)
/// 4. **Fragments**: Removed (e.g., `#section`)
/// 5. **Query parameters**: Preserved as-is
/// 6. **Path**: Preserved with case sensitivity
///
/// # Security
///
/// Rejects potentially dangerous protocols like `javascript:`, `data:`, `file:`, etc.
///
/// # Errors
///
/// Returns [`UrlNormalizationError::InvalidFormat`] for malformed URLs.
/// Returns [`UrlNormalizationError::UnsupportedProtocol`] for non-HTTP(S) schemes.
///
/// # Examples
///
/// ```ignore
/// // Case normalization
/// assert_eq!(
///     normalize_url("HTTPS://EXAMPLE.COM/Path").unwrap(),
///     "https://example.com/Path"
/// );
///
/// // Default port removal
/// assert_eq!(
///     normalize_url("https://example.com:443/path").unwrap(),
///     "https://example.com/path"
/// );
///
/// // Fragment removal
/// assert_eq!(
///     normalize_url("https://example.com/page#section").unwrap(),
///     "https://example.com/page"
/// );
/// ```
pub fn normalize_url(input: &str) -> Result<String, UrlNormalizationError> {
    let mut url =
        Url::parse(input).map_err(|e| UrlNormalizationError::InvalidFormat(e.to_string()))?;

    match url.scheme() {
        "http" | "https" => {}
        _ => return Err(UrlNormalizationError::UnsupportedProtocol),
    }

    if let Some(host) = url.host_str() {
        let host_lowercase = host.to_ascii_lowercase();
        url.set_host(Some(&host_lowercase)).map_err(|_| {
            UrlNormalizationError::NormalizationFailed("Failed to set normalized host".to_string())
        })?;
    }

    url.set_fragment(None);

    let is_default_port = matches!(
        (url.scheme(), url.port()),
        ("http", Some(80)) | ("https", Some(443))
    );
    if is_default_port {
        url.set_port(None).map_err(|_| {
            UrlNormalizationError::NormalizationFailed("Failed to remove default port".to_string())
        })?;
    }

    Ok(url.to_string())
}

/// Returns `true` if the URL targets a publicly-routable host.
///
/// This guards against the shortener being used to redirect to internal
/// infrastructure: loopback, RFC 1918 private ranges, link-local, carrier-grade
/// NAT, IPv6 unique-local/link-local addresses, and the `localhost` domain are
/// all treated as non-public. The input is expected to be a normalized http(s)
/// URL (see [`normalize_url`]); anything unparseable is treated as non-public.
pub fn is_public_url(input: &str) -> bool {
    let Ok(url) = Url::parse(input) else {
        return false;
    };
    match url.host() {
        Some(Host::Domain(domain)) => is_public_domain(domain),
        Some(Host::Ipv4(ip)) => is_public_ipv4(ip),
        Some(Host::Ipv6(ip)) => is_public_ipv6(ip),
        None => false,
    }
}

/// Rejects `localhost` and any `*.localhost` subdomain.
fn is_public_domain(domain: &str) -> bool {
    let d = domain.trim_end_matches('.').to_ascii_lowercase();
    !(d == "localhost" || d.ends_with(".localhost"))
}

/// Rejects loopback, private, link-local, unspecified, broadcast, documentation
/// and carrier-grade-NAT (100.64.0.0/10) IPv4 addresses.
fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    let is_cgnat = o[0] == 100 && (o[1] & 0xc0) == 0x40; // 100.64.0.0/10
    !(ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_documentation()
        || is_cgnat)
}

/// Rejects loopback/unspecified, IPv6 unique-local (fc00::/7) and link-local
/// (fe80::/10), plus any address embedding a non-public IPv4 (mapped/compat).
fn is_public_ipv6(ip: Ipv6Addr) -> bool {
    if ip.is_loopback() || ip.is_unspecified() {
        return false;
    }
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_public_ipv4(v4);
    }
    if let Some(v4) = ip.to_ipv4() {
        return is_public_ipv4(v4);
    }
    let seg = ip.segments();
    let is_unique_local = (seg[0] & 0xfe00) == 0xfc00; // fc00::/7
    let is_link_local = (seg[0] & 0xffc0) == 0xfe80; // fe80::/10
    !(is_unique_local || is_link_local)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_simple_http() {
        let result = normalize_url("http://example.com");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "http://example.com/");
    }

    #[test]
    fn test_normalize_simple_https() {
        let result = normalize_url("https://example.com");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "https://example.com/");
    }

    #[test]
    fn test_normalize_uppercase_host() {
        let result = normalize_url("https://EXAMPLE.COM/path");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "https://example.com/path");
    }

    #[test]
    fn test_normalize_mixed_case_host() {
        let result = normalize_url("https://ExAmPlE.CoM");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "https://example.com/");
    }

    #[test]
    fn test_normalize_remove_default_http_port() {
        let result = normalize_url("http://example.com:80/path");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "http://example.com/path");
    }

    #[test]
    fn test_normalize_remove_default_https_port() {
        let result = normalize_url("https://example.com:443/path");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "https://example.com/path");
    }

    #[test]
    fn test_normalize_keep_custom_port() {
        let result = normalize_url("http://example.com:8080/path");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "http://example.com:8080/path");
    }

    #[test]
    fn test_normalize_remove_fragment() {
        let result = normalize_url("https://example.com/page#section");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "https://example.com/page");
    }

    #[test]
    fn test_normalize_remove_fragment_with_query() {
        let result = normalize_url("https://example.com/page?key=value#section");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "https://example.com/page?key=value");
    }

    #[test]
    fn test_normalize_preserve_query_params() {
        let result = normalize_url("https://example.com/search?q=rust&lang=en");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "https://example.com/search?q=rust&lang=en");
    }

    #[test]
    fn test_normalize_preserve_path() {
        let result = normalize_url("https://example.com/path/to/page");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "https://example.com/path/to/page");
    }

    #[test]
    fn test_normalize_complex_url() {
        let result = normalize_url("HTTPS://EXAMPLE.COM:443/Path?key=VALUE#anchor");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "https://example.com/Path?key=VALUE");
    }

    #[test]
    fn test_normalize_trailing_slash() {
        let result = normalize_url("https://example.com/");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "https://example.com/");
    }

    #[test]
    fn test_normalize_subdomain() {
        let result = normalize_url("https://api.example.com/v1/users");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "https://api.example.com/v1/users");
    }

    #[test]
    fn test_normalize_with_authentication() {
        let result = normalize_url("https://user:pass@example.com/path");
        assert!(result.is_ok());
        assert!(result.unwrap().contains("user:pass"));
    }

    #[test]
    fn test_normalize_ip_address() {
        let result = normalize_url("http://192.168.1.1:8080/api");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "http://192.168.1.1:8080/api");
    }

    #[test]
    fn test_normalize_localhost() {
        let result = normalize_url("http://localhost:3000/test");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "http://localhost:3000/test");
    }

    #[test]
    fn test_normalize_invalid_url() {
        let result = normalize_url("not a valid url");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            UrlNormalizationError::InvalidFormat(_)
        ));
    }

    #[test]
    fn test_normalize_ftp_protocol() {
        let result = normalize_url("ftp://example.com/file.txt");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            UrlNormalizationError::UnsupportedProtocol
        ));
    }

    #[test]
    fn test_normalize_file_protocol() {
        let result = normalize_url("file:///home/user/document.txt");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            UrlNormalizationError::UnsupportedProtocol
        ));
    }

    #[test]
    fn test_normalize_empty_string() {
        let result = normalize_url("");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            UrlNormalizationError::InvalidFormat(_)
        ));
    }

    #[test]
    fn test_normalize_no_protocol() {
        let result = normalize_url("example.com");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            UrlNormalizationError::InvalidFormat(_)
        ));
    }

    #[test]
    fn test_normalize_invalid_characters() {
        let result = normalize_url("https://example.com/<invalid>");
        let _ = result;
    }

    #[test]
    fn test_normalize_javascript_protocol() {
        let result = normalize_url("javascript:alert('xss')");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            UrlNormalizationError::UnsupportedProtocol
        ));
    }

    #[test]
    fn test_normalize_data_protocol() {
        let result = normalize_url("data:text/plain,Hello");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            UrlNormalizationError::UnsupportedProtocol
        ));
    }

    #[test]
    fn test_normalize_mailto_protocol() {
        let result = normalize_url("mailto:test@example.com");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            UrlNormalizationError::UnsupportedProtocol
        ));
    }

    #[test]
    fn test_normalize_very_long_url() {
        let long_path = "a".repeat(2000);
        let url = format!("https://example.com/{}", long_path);
        let result = normalize_url(&url);
        assert!(result.is_ok());
        assert!(result.unwrap().len() > 2000);
    }

    #[test]
    fn test_normalize_multiple_query_params() {
        let result = normalize_url("https://example.com/search?a=1&b=2&c=3&d=4");
        assert!(result.is_ok());
        let normalized = result.unwrap();
        assert!(normalized.contains("a=1"));
        assert!(normalized.contains("b=2"));
    }

    #[test]
    fn test_normalize_encoded_characters() {
        let result = normalize_url("https://example.com/path%20with%20spaces");
        assert!(result.is_ok());
        assert!(result.unwrap().contains("path%20with%20spaces"));
    }

    #[test]
    fn test_normalize_unicode_domain() {
        let result = normalize_url("https://münchen.de");
        assert!(result.is_ok());
    }

    // ── is_public_url ─────────────────────────────────────────────────────────

    #[test]
    fn test_is_public_url_allows_public_domain() {
        assert!(is_public_url("https://example.com/path"));
        assert!(is_public_url("http://sub.example.org"));
        assert!(is_public_url("https://münchen.de"));
    }

    #[test]
    fn test_is_public_url_allows_public_ip() {
        assert!(is_public_url("http://8.8.8.8"));
        assert!(is_public_url("https://[2606:4700:4700::1111]"));
    }

    #[test]
    fn test_is_public_url_blocks_localhost() {
        assert!(!is_public_url("http://localhost"));
        assert!(!is_public_url("http://localhost:3000/admin"));
        assert!(!is_public_url("http://api.localhost"));
    }

    #[test]
    fn test_is_public_url_blocks_loopback_ipv4() {
        assert!(!is_public_url("http://127.0.0.1"));
        assert!(!is_public_url("http://127.0.0.1:8080/internal"));
    }

    #[test]
    fn test_is_public_url_blocks_private_ipv4() {
        assert!(!is_public_url("http://10.0.0.5"));
        assert!(!is_public_url("http://172.16.0.1"));
        assert!(!is_public_url("http://192.168.1.1"));
    }

    #[test]
    fn test_is_public_url_blocks_link_local_and_cgnat() {
        assert!(!is_public_url("http://169.254.169.254/latest/meta-data")); // cloud metadata
        assert!(!is_public_url("http://100.64.0.1"));
    }

    #[test]
    fn test_is_public_url_blocks_unspecified() {
        assert!(!is_public_url("http://0.0.0.0"));
    }

    #[test]
    fn test_is_public_url_blocks_ipv6_loopback_and_ula() {
        assert!(!is_public_url("http://[::1]"));
        assert!(!is_public_url("http://[fd00::1]")); // unique local
        assert!(!is_public_url("http://[fe80::1]")); // link local
    }

    #[test]
    fn test_is_public_url_blocks_ipv4_mapped_loopback() {
        assert!(!is_public_url("http://[::ffff:127.0.0.1]"));
    }

    #[test]
    fn test_is_public_url_rejects_unparseable() {
        assert!(!is_public_url("not a url"));
    }
}
