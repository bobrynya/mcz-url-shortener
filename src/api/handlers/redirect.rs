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
/// 5. Asynchronously update cache with redirect-type prefix
/// 6. Send click event to background worker
/// 7. Return 301 Permanent or 307 Temporary redirect based on link's `permanent` flag
///
/// # Cache Encoding
///
/// Cached values are prefixed to preserve the redirect type:
/// - `"1:{url}"` → 301 Permanent Redirect
/// - `"0:{url}"` → 307 Temporary Redirect
/// - No prefix (legacy) → 307 Temporary Redirect
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

    let (long_url, permanent) = match state.cache.get_url(&cache_key).await {
        Ok(Some(cached_value)) => {
            debug!("Cache HIT for {}", cache_key);
            parse_cached_value(&cached_value)
        }
        Ok(None) => {
            debug!("Cache MISS for {}", cache_key);

            let link = load_active_link(&state, &domain, &code).await?;
            let url = link.long_url.clone();
            let permanent = link.permanent;

            // Cache with redirect-type prefix. Use expiry-aware TTL if applicable.
            let cache_clone = state.cache.clone();
            let cache_key_clone = cache_key.clone();
            let ttl = link.expires_at.map(|exp| {
                let secs = (exp - chrono::Utc::now()).num_seconds();
                secs.max(1) as usize
            });
            let cached_value = encode_cached_value(&url, permanent);
            tokio::spawn(async move {
                if let Err(e) = cache_clone
                    .set_url(&cache_key_clone, &cached_value, ttl)
                    .await
                {
                    error!("Failed to cache URL: {}", e);
                }
            });

            (url, permanent)
        }
        Err(e) => {
            error!("Cache error: {}", e);

            // Fall back to database on cache error.
            let link = load_active_link(&state, &domain, &code).await?;
            (link.long_url, link.permanent)
        }
    };

    // Send click event for async processing.
    let click_event = ClickEvent::new(
        domain,
        code,
        Some(addr.ip().to_string()),
        headers
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok()),
        headers.get(header::REFERER).and_then(|v| v.to_str().ok()),
    );

    if state.click_sender.try_send(click_event).is_err() {
        // Queue is full or closed — the click is dropped rather than blocking
        // the redirect. Surface it via metrics so saturation is observable.
        metrics::counter!("click_enqueue_dropped_total").increment(1);
        debug!("Click queue full; dropped click event for {}", cache_key);
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

/// Encodes a URL with a redirect-type prefix for caching.
fn encode_cached_value(url: &str, permanent: bool) -> String {
    if permanent {
        format!("{}{}", PERMANENT_PREFIX, url)
    } else {
        format!("{}{}", TEMPORARY_PREFIX, url)
    }
}

/// Parses a cached value, extracting the URL and redirect type.
///
/// Handles both prefixed (new) and legacy (no prefix) entries.
fn parse_cached_value(value: &str) -> (String, bool) {
    if let Some(url) = value.strip_prefix(PERMANENT_PREFIX) {
        (url.to_string(), true)
    } else if let Some(url) = value.strip_prefix(TEMPORARY_PREFIX) {
        (url.to_string(), false)
    } else {
        // Legacy cached entries without prefix → treat as temporary.
        (value.to_string(), false)
    }
}
