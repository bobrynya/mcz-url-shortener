//! Login page and session cookie handlers.

use askama::Template;
use askama_web::WebTemplate;
use axum::{
    Json,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use crate::state::AppState;

/// Name of the dashboard session cookie.
const AUTH_COOKIE: &str = "auth_token";
/// Session cookie lifetime in seconds (30 days).
const COOKIE_MAX_AGE: i64 = 30 * 24 * 60 * 60;

/// Template for the login page.
///
/// Renders `templates/login.html` with:
/// - Token input form
/// - Authentication instructions
#[derive(Template, WebTemplate)]
#[template(path = "login.html")]
struct LoginTemplate {}

/// Renders the login page.
///
/// # Endpoint
///
/// `GET /dashboard/login`
pub async fn login_handler() -> impl IntoResponse {
    LoginTemplate {}
}

/// Request body for the login submission endpoint.
#[derive(Deserialize)]
pub struct LoginRequest {
    /// The API token to validate and store in the session cookie.
    pub token: String,
}

/// Validates a token and, on success, sets the session cookie.
///
/// # Endpoint
///
/// `POST /dashboard/login`
///
/// # Security
///
/// The token is stored in an **HttpOnly** cookie set server-side, so it is never
/// exposed to JavaScript (mitigating token theft via XSS). The cookie is also
/// `SameSite=Lax` and — when `cookie_secure` is enabled — `Secure` (HTTPS-only).
///
/// # Responses
///
/// - `204 No Content` with `Set-Cookie` on success
/// - `401 Unauthorized` if the token is invalid or revoked
pub async fn login_submit_handler(
    State(state): State<AppState>,
    Json(payload): Json<LoginRequest>,
) -> Response {
    match state.auth_service.authenticate(&payload.token).await {
        Ok(()) => {
            let cookie = build_session_cookie(&payload.token, COOKIE_MAX_AGE, state.cookie_secure);
            (StatusCode::NO_CONTENT, [(header::SET_COOKIE, cookie)]).into_response()
        }
        Err(e) => e.into_response(),
    }
}

/// Clears the session cookie, logging the user out.
///
/// # Endpoint
///
/// `POST /dashboard/logout`
///
/// Because the session cookie is `HttpOnly`, it cannot be cleared from
/// JavaScript — logout must go through the server, which overwrites the cookie
/// with an immediately-expiring one.
pub async fn logout_handler(State(state): State<AppState>) -> Response {
    let cookie = build_session_cookie("", 0, state.cookie_secure);
    (StatusCode::NO_CONTENT, [(header::SET_COOKIE, cookie)]).into_response()
}

/// Builds a `Set-Cookie` value for the session cookie.
///
/// `max_age` of `0` (with an empty value) clears the cookie. The token charset
/// produced by the admin CLI is alphanumeric, so no extra encoding is required.
fn build_session_cookie(token: &str, max_age: i64, secure: bool) -> String {
    let mut cookie =
        format!("{AUTH_COOKIE}={token}; Path=/; Max-Age={max_age}; HttpOnly; SameSite=Lax");
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_session_cookie_secure() {
        let c = build_session_cookie("abc123", COOKIE_MAX_AGE, true);
        assert!(c.starts_with("auth_token=abc123;"));
        assert!(c.contains("HttpOnly"));
        assert!(c.contains("SameSite=Lax"));
        assert!(c.contains("Secure"));
        assert!(c.contains("Max-Age=2592000"));
    }

    #[test]
    fn test_build_session_cookie_insecure_omits_secure() {
        let c = build_session_cookie("abc123", COOKIE_MAX_AGE, false);
        assert!(c.contains("HttpOnly"));
        assert!(!c.contains("Secure"));
    }

    #[test]
    fn test_build_session_cookie_clear() {
        let c = build_session_cookie("", 0, true);
        assert!(c.contains("Max-Age=0"));
        assert!(c.starts_with("auth_token=;"));
    }
}
