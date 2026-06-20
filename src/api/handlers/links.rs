//! Handlers for link management endpoints (create, update, delete).

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::json;
use validator::Validate;

use crate::api::dto::batch_links::{
    BatchDeactivateResponse, BatchDeactivateSummary, BatchLinkItem, BatchLinksRequest,
    BatchRestoreResponse, BatchRestoreSummary,
};
use crate::api::dto::shorten::{
    BatchSummary, ShortenRequest, ShortenResponse, ShortenResultItem, UrlItem,
};
use crate::api::dto::update_link::UpdateLinkRequest;
use crate::domain::entities::LinkPatch;
use crate::error::AppError;
use crate::state::AppState;
/// Query parameters for `DELETE /api/links/{code}`.
#[derive(Debug, serde::Deserialize)]
pub struct DeleteLinkQuery {
    /// Target domain id. When omitted, the default domain is used.
    pub domain_id: Option<i64>,
}

/// JSON representation of a link returned after update.
#[derive(Debug, Serialize)]
pub struct LinkResponse {
    pub code: String,
    pub long_url: String,
    pub short_url: String,
    pub permanent: bool,
    pub expires_at: Option<DateTime<Utc>>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Creates shortened URLs for one or more long URLs.
///
/// # Endpoint
///
/// `POST /api/shorten`
///
/// # Batch Processing
///
/// Processes URLs independently. If one fails, others continue processing.
/// Each result includes either success data or error information.
///
/// # Request Body
///
/// ```json
/// {
///   "urls": [
///     {
///       "url": "https://example.com",
///       "domain_id": 1,  // optional
///       "custom_code": "my-link"     // optional
///     }
///   ]
/// }
/// ```
///
/// # Errors
///
/// Returns 400 Bad Request if validation fails.
/// Individual URL errors are returned in the response items array.
pub async fn shorten_handler(
    State(state): State<AppState>,
    Json(payload): Json<ShortenRequest>,
) -> Result<Json<ShortenResponse>, AppError> {
    payload.validate()?;

    let total = payload.urls.len();
    let mut results = Vec::with_capacity(total);
    let mut successful = 0;
    let mut failed = 0;

    for item in payload.urls {
        let long_url = item.url.clone();

        match process_single_url(&state, item).await {
            Ok((code, short_url)) => {
                successful += 1;
                results.push(ShortenResultItem::Success {
                    long_url,
                    code,
                    short_url,
                });
            }
            Err(err) => {
                failed += 1;
                results.push(ShortenResultItem::Error {
                    long_url,
                    error: err.to_error_info(),
                });
            }
        }
    }

    Ok(Json(ShortenResponse {
        summary: BatchSummary {
            total,
            successful,
            failed,
        },
        items: results,
    }))
}

/// Resolves the target domain, creates the short link, and generates the full URL.
async fn process_single_url(state: &AppState, item: UrlItem) -> Result<(String, String), AppError> {
    let domain = if let Some(domain_id) = item.domain_id {
        state.domain_service.get_domain_by_id(domain_id).await?
    } else {
        state.domain_service.get_default_domain().await?
    };

    let link = state
        .link_service
        .create_short_link_for_domain(
            item.url,
            domain.id,
            item.custom_code,
            item.expires_at,
            item.permanent.unwrap_or(false),
        )
        .await?;

    let short_url = state.link_service.get_short_url(&domain.domain, &link.code);

    Ok((link.code, short_url))
}

/// Partially updates a short link.
///
/// # Endpoint
///
/// `PATCH /api/links/{code}`
///
/// # Request Body
///
/// All fields are optional. Only provided fields are changed.
///
/// ```json
/// {
///   "url": "https://new-destination.com",
///   "expires_at": "2026-12-31T23:59:59Z",  // null to clear
///   "permanent": true,
///   "restore": true   // clears deleted_at to un-delete the link
/// }
/// ```
///
/// # Cache
///
/// The cache entry for this link is invalidated so the next redirect uses the
/// updated destination and redirect type.
///
/// # Errors
///
/// Returns 404 Not Found if the link doesn't exist for this domain.
/// Returns 400 Bad Request if validation fails.
pub async fn update_link_handler(
    Path(code): Path<String>,
    State(state): State<AppState>,
    Json(payload): Json<UpdateLinkRequest>,
) -> Result<Json<LinkResponse>, AppError> {
    payload.validate()?;

    let domain_entity = match payload.domain_id {
        Some(id) => state.domain_service.get_domain_by_id(id).await?,
        None => state.domain_service.get_default_domain().await?,
    };
    let domain = domain_entity.domain.clone();

    let patch = LinkPatch {
        url: payload.url,
        expires_at: payload.expires_at,
        permanent: payload.permanent,
        restore: payload.restore,
    };

    let link = state
        .link_service
        .update_link(&code, domain_entity.id, patch)
        .await?;

    let cache_key = format!("{}:{}", domain, code);
    if let Err(e) = state.cache.invalidate(&cache_key).await {
        tracing::warn!(error = ?e, cache_key, "Failed to invalidate cache after update");
    }

    let short_url = state.link_service.get_short_url(&domain, &link.code);

    Ok(Json(LinkResponse {
        code: link.code,
        long_url: link.long_url,
        short_url,
        permanent: link.permanent,
        expires_at: link.expires_at,
        deleted_at: link.deleted_at,
        created_at: link.created_at,
    }))
}

/// Soft-deletes a short link by setting its `deleted_at` timestamp.
///
/// # Endpoint
///
/// `DELETE /api/links/{code}`
///
/// # Behavior
///
/// - The link record is **not** removed from the database. `deleted_at` is set to now.
/// - Subsequent redirect requests for this code will return **410 Gone**.
/// - A deleted link can be restored via `PATCH /api/links/{code}` with `{"restore": true}`.
///
/// # Cache
///
/// The cache entry for this link is invalidated immediately so the next redirect
/// reflects the deleted state without waiting for TTL expiry.
///
/// # Errors
///
/// Returns 404 Not Found if the link doesn't exist or is already deleted.
pub async fn delete_link_handler(
    Path(code): Path<String>,
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<DeleteLinkQuery>,
) -> Result<StatusCode, AppError> {
    let domain_entity = match query.domain_id {
        Some(id) => state.domain_service.get_domain_by_id(id).await?,
        None => state.domain_service.get_default_domain().await?,
    };
    let domain = domain_entity.domain.clone();

    let deleted = state
        .link_service
        .soft_delete_link(&code, domain_entity.id)
        .await?;

    if !deleted {
        return Err(AppError::not_found(
            "Link not found or already deleted",
            json!({ "code": code }),
        ));
    }

    let cache_key = format!("{}:{}", domain, code);
    if let Err(e) = state.cache.invalidate(&cache_key).await {
        tracing::warn!(error = ?e, cache_key, "Failed to invalidate cache after delete");
    }

    Ok(StatusCode::NO_CONTENT)
}

/// Resolves the target domain for a batch request: an explicit `domain_id` (404
/// if unknown), or the default domain when omitted.
async fn resolve_batch_domain(
    state: &AppState,
    domain_id: Option<i64>,
) -> Result<crate::domain::entities::Domain, AppError> {
    match domain_id {
        Some(id) => state.domain_service.get_domain_by_id(id).await,
        None => state.domain_service.get_default_domain().await,
    }
}

/// Builds per-code items in original (de-duplicated) input order, marking each
/// code `affected_status` when present in `affected`, else `not_found`.
fn build_batch_items(
    requested: &[String],
    affected: &[String],
    affected_status: &str,
) -> Vec<BatchLinkItem> {
    let affected_set: std::collections::HashSet<&String> = affected.iter().collect();
    let mut seen = std::collections::HashSet::new();
    requested
        .iter()
        .filter(|c| seen.insert((*c).clone()))
        .map(|code| BatchLinkItem {
            code: code.clone(),
            status: if affected_set.contains(code) {
                affected_status.to_string()
            } else {
                "not_found".to_string()
            },
        })
        .collect()
}

/// Bulk-deactivates (soft-deletes) short links.
///
/// `POST /api/links/batch-deactivate` — body `{ "codes": [...], "domain_id"?: i64 }`.
/// Returns HTTP 200 with a per-code summary; missing or already-deleted codes are
/// reported as `not_found`. Idempotent.
pub async fn batch_deactivate_handler(
    State(state): State<AppState>,
    Json(payload): Json<BatchLinksRequest>,
) -> Result<Json<BatchDeactivateResponse>, AppError> {
    payload.validate()?;
    let domain = resolve_batch_domain(&state, payload.domain_id).await?;

    let affected = state
        .link_service
        .deactivate_links(payload.codes.clone(), domain.id)
        .await?;

    for code in &affected {
        let cache_key = format!("{}:{}", domain.domain, code);
        if let Err(e) = state.cache.invalidate(&cache_key).await {
            tracing::warn!(error = ?e, cache_key, "Failed to invalidate cache after batch deactivate");
        }
    }

    let items = build_batch_items(&payload.codes, &affected, "deactivated");
    Ok(Json(BatchDeactivateResponse {
        summary: BatchDeactivateSummary {
            total: items.len(),
            deactivated: affected.len(),
            not_found: items.len() - affected.len(),
        },
        items,
    }))
}

/// Bulk-restores soft-deleted short links.
///
/// `POST /api/links/batch-restore` — body `{ "codes": [...], "domain_id"?: i64 }`.
/// Returns HTTP 200 with a per-code summary; missing or already-active codes are
/// reported as `not_found`. Idempotent.
pub async fn batch_restore_handler(
    State(state): State<AppState>,
    Json(payload): Json<BatchLinksRequest>,
) -> Result<Json<BatchRestoreResponse>, AppError> {
    payload.validate()?;
    let domain = resolve_batch_domain(&state, payload.domain_id).await?;

    let affected = state
        .link_service
        .restore_links(payload.codes.clone(), domain.id)
        .await?;

    for code in &affected {
        let cache_key = format!("{}:{}", domain.domain, code);
        if let Err(e) = state.cache.invalidate(&cache_key).await {
            tracing::warn!(error = ?e, cache_key, "Failed to invalidate cache after batch restore");
        }
    }

    let items = build_batch_items(&payload.codes, &affected, "restored");
    Ok(Json(BatchRestoreResponse {
        summary: BatchRestoreSummary {
            total: items.len(),
            restored: affected.len(),
            not_found: items.len() - affected.len(),
        },
        items,
    }))
}
