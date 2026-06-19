//! Bearer token authentication middleware.

use axum::{
    extract::{FromRequestParts, Request, State},
    http::header::COOKIE,
    http::request::Parts,
    middleware::Next,
    response::Response,
};
use axum_auth::AuthBearer;

use crate::{error::AppError, state::AppState};

/// Name of the dashboard session cookie used as an authentication fallback.
const AUTH_COOKIE: &str = "auth_token";

/// Extracts the `auth_token` value from the `Cookie` header, if present.
///
/// The dashboard stores the API token in an `HttpOnly` cookie (see
/// [`crate::web::handlers`]), so same-origin browser calls to the API
/// authenticate via this cookie rather than an `Authorization` header.
fn token_from_cookie(parts: &Parts) -> Option<String> {
    parts
        .headers
        .get(COOKIE)
        .and_then(|header| header.to_str().ok())
        .and_then(|cookie_str| {
            cookie_str.split(';').find_map(|cookie| {
                let mut kv = cookie.trim().splitn(2, '=');
                match (kv.next(), kv.next()) {
                    (Some(AUTH_COOKIE), Some(value)) => Some(value.to_string()),
                    _ => None,
                }
            })
        })
}

/// Authenticates requests using Bearer tokens from Authorization header.
///
/// # Header Format
///
/// ```text
/// Authorization: Bearer <token>
/// ```
///
/// # Authentication Flow
///
/// 1. Extract token from `Authorization` header
/// 2. Validate token hash against database
/// 3. Check if token is revoked
/// 4. Update `last_used_at` timestamp
/// 5. Continue to next middleware/handler
///
/// # Errors
///
/// Returns `401 Unauthorized` if:
/// - Authorization header is missing
/// - Token format is invalid
/// - Token is not found or revoked
///
/// Adds `WWW-Authenticate: Bearer` header to 401 responses per RFC 6750.
///
/// # Example
///
/// ```rust,ignore
/// use axum::{Router, routing::get, middleware};
/// use crate::api::middleware::auth;
///
/// let protected = Router::new()
///     .route("/api/stats", get(stats_handler))
///     .layer(middleware::from_fn_with_state(state.clone(), auth::layer));
/// ```
pub async fn layer(
    State(st): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, AppError> {
    let (mut parts, body) = req.into_parts();

    // Prefer the `Authorization: Bearer` header (external API clients), then fall
    // back to the `auth_token` cookie (same-origin dashboard requests).
    let token = match AuthBearer::from_request_parts(&mut parts, &()).await {
        Ok(AuthBearer(token)) => token,
        Err(_) => token_from_cookie(&parts).ok_or_else(|| {
            AppError::unauthorized(
                "Unauthorized",
                serde_json::json!({"reason": "Authorization header is missing or invalid"}),
            )
        })?,
    };

    let req = Request::from_parts(parts, body);

    st.auth_service.authenticate(&token).await?;

    Ok(next.run(req).await)
}
