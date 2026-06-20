//! Handler for short URL redirect.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Redirect},
};
use serde_json::json;
use std::net::SocketAddr;
use tracing::{debug, error};

use crate::domain::click_event::ClickEvent;
use crate::domain::entities::Link;
use crate::error::AppError;
use crate::state::AppState;
use crate::utils::extract_domain::extract_domain_from_headers;

/// Cache value prefix for permanent (301) links.
const PERMANENT_PREFIX: &str = "1:";
/// Cache value prefix for temporary (307) links.
const TEMPORARY_PREFIX: &str = "0:";

/// Redirects a short code to its original URL.
///
/// # Endpoint
///
/// `GET /{code}`
///
/// # Request Flow
///
/// 1. Extract domain from Host header
/// 2. Check cache for URL (cache key: `domain:code`)
/// 3. On cache miss, query database
/// 4. Check if link is deleted or expired → 410 Gone
/// 5. Asynchronously update cache with redirect-type prefix and link id
/// 6. Publish click event to Kafka (fire-and-forget)
/// 7. Return 301 Permanent or 307 Temporary redirect based on link's `permanent` flag
///
/// # Cache Encoding
///
/// Cached values are prefixed to preserve the redirect type and carry the link id:
/// - `"1:{id}|{url}"` → 301 Permanent Redirect
/// - `"0:{id}|{url}"` → 307 Temporary Redirect
/// - No prefix (legacy) → 307 Temporary Redirect, `link_id = 0` (click skipped)
///
/// # Errors
///
/// Returns 404 Not Found if the short code doesn't exist.
/// Returns 410 Gone if the link has been deleted or has expired.
/// Returns 400 Bad Request if the Host header is missing or invalid.
pub async fn redirect_handler(
    Path(code): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<SocketAddr>,
) -> Result<impl IntoResponse, AppError> {
    let domain = extract_domain_from_headers(&headers)?;

    let cache_key = format!("{}:{}", domain, code);

    let (long_url, permanent, link_id) = match state.cache.get_url(&cache_key).await {
        Ok(Some(cached_value)) => {
            metrics::counter!("cache_requests_total", "result" => "hit").increment(1);
            debug!("Cache HIT for {}", cache_key);
            let (link_id, url, permanent) = parse_cached_value(&cached_value);
            (url, permanent, link_id)
        }
        Ok(None) => {
            metrics::counter!("cache_requests_total", "result" => "miss").increment(1);
            debug!("Cache MISS for {}", cache_key);

            let link = load_active_link(&state, &domain, &code).await?;
            let url = link.long_url.clone();
            let permanent = link.permanent;
            let link_id = link.id;

            // Cache with redirect-type prefix. Use expiry-aware TTL if applicable.
            let cache_clone = state.cache.clone();
            let cache_key_clone = cache_key.clone();
            let ttl = link.expires_at.map(|exp| {
                let secs = (exp - chrono::Utc::now()).num_seconds();
                secs.max(1) as usize
            });
            let cached_value = encode_cached_value(link_id, &url, permanent);
            tokio::spawn(async move {
                if let Err(e) = cache_clone
                    .set_url(&cache_key_clone, &cached_value, ttl)
                    .await
                {
                    error!("Failed to cache URL: {}", e);
                }
            });

            (url, permanent, link_id)
        }
        Err(e) => {
            metrics::counter!("cache_requests_total", "result" => "error").increment(1);
            error!("Cache error: {}", e);

            // Fall back to database on cache error.
            let link = load_active_link(&state, &domain, &code).await?;
            (link.long_url, link.permanent, link.id)
        }
    };

    // Publish click event for async processing (fire-and-forget).
    if link_id > 0 {
        let event = ClickEvent::new(
            link_id,
            Some(addr.ip().to_string()),
            headers
                .get(header::USER_AGENT)
                .and_then(|v| v.to_str().ok())
                .map(String::from),
            headers
                .get(header::REFERER)
                .and_then(|v| v.to_str().ok())
                .map(String::from),
            chrono::Utc::now(),
        );
        let publisher = state.click_publisher.clone();
        tokio::spawn(async move {
            let _ = publisher.publish(event).await;
        });
    }

    if permanent {
        Ok(Redirect::permanent(&long_url))
    } else {
        Ok(Redirect::temporary(&long_url))
    }
}

/// Loads a link from the database and rejects it if it is no longer servable.
///
/// Resolves the domain, fetches the link by code, and returns [`AppError::Gone`]
/// if the link has been soft-deleted (takes precedence) or has expired.
async fn load_active_link(state: &AppState, domain: &str, code: &str) -> Result<Link, AppError> {
    let domain_entity = state.domain_service.get_domain(domain).await?;
    let link = state
        .link_service
        .get_link_by_code(code, domain_entity.id)
        .await?;

    // Deleted takes precedence over expired in the error message.
    if link.is_deleted() {
        return Err(AppError::gone(
            "This link has been deleted",
            json!({ "code": code }),
        ));
    }
    if link.is_expired() {
        return Err(AppError::gone(
            "This link has expired",
            json!({ "code": code }),
        ));
    }

    Ok(link)
}

/// Encodes `link_id` + URL with a redirect-type prefix for caching: `"{1:|0:}{id}|{url}"`.
fn encode_cached_value(link_id: i64, url: &str, permanent: bool) -> String {
    let p = if permanent {
        PERMANENT_PREFIX
    } else {
        TEMPORARY_PREFIX
    };
    format!("{}{}|{}", p, link_id, url)
}

/// Parses a cached value into `(link_id, url, permanent)`.
///
/// Handles both prefixed (new) and legacy (no prefix) entries.
/// Legacy entries without an id yield `link_id = 0` (click is skipped).
fn parse_cached_value(value: &str) -> (i64, String, bool) {
    let (permanent, rest) = if let Some(r) = value.strip_prefix(PERMANENT_PREFIX) {
        (true, r)
    } else if let Some(r) = value.strip_prefix(TEMPORARY_PREFIX) {
        (false, r)
    } else {
        (false, value)
    };
    match rest.split_once('|') {
        Some((id, url)) => (id.parse().unwrap_or(0), url.to_string(), permanent),
        None => (0, rest.to_string(), permanent),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_value_round_trip_permanent() {
        let encoded = encode_cached_value(42, "https://example.com", true);
        let (id, url, permanent) = parse_cached_value(&encoded);
        assert_eq!(id, 42);
        assert_eq!(url, "https://example.com");
        assert!(permanent);
    }

    #[test]
    fn test_cache_value_round_trip_temporary() {
        let encoded = encode_cached_value(7, "https://example.org/path|with|pipes", false);
        let (id, url, permanent) = parse_cached_value(&encoded);
        assert_eq!(id, 7);
        assert_eq!(url, "https://example.org/path|with|pipes");
        assert!(!permanent);
    }

    #[test]
    fn test_parse_legacy_value_without_id() {
        // Legacy entries (no prefix, no id) → link_id 0, temporary.
        let (id, url, permanent) = parse_cached_value("https://legacy.example.com");
        assert_eq!(id, 0);
        assert_eq!(url, "https://legacy.example.com");
        assert!(!permanent);
    }
}
