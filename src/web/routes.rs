//! Web dashboard route configuration.

use crate::state::AppState;
use crate::web::handlers::{
    dashboard_handler, domains_handler, links_handler, login_handler, login_submit_handler,
    logout_handler, stats_handler,
};
use axum::{
    Router,
    routing::{get, post},
};

/// Protected dashboard routes requiring authentication.
///
/// Protected via [`crate::web::middleware::web_auth`] (cookie-based or similar).
///
/// # Endpoints
///
/// - `GET /` - Dashboard home with overview
/// - `GET /links` - Link management page
/// - `GET /stats/{code}` - Detailed statistics page for a specific link
/// - `GET /domains` - Domain management page
pub fn protected_routes() -> Router<AppState> {
    Router::new()
        .route("/", get(dashboard_handler))
        .route("/links", get(links_handler))
        .route("/stats/{code}", get(stats_handler))
        .route("/domains", get(domains_handler))
}

/// Public dashboard routes without authentication.
///
/// # Endpoints
///
/// - `GET  /login`  - Login page
/// - `POST /login`  - Validate a token and set the HttpOnly session cookie
/// - `POST /logout` - Clear the session cookie
pub fn public_routes() -> Router<AppState> {
    Router::new()
        .route("/login", get(login_handler).post(login_submit_handler))
        .route("/logout", post(logout_handler))
}
