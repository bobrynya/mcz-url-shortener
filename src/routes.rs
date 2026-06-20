//! Top-level router configuration combining API and web routes.
//!
//! # Route Structure
//!
//! - `GET  /{code}`      - Short link redirect (public)
//! - `GET  /health`      - Health check: DB, cache, click queue (public)
//! - `/api/*`            - REST API (Bearer token required)
//! - `/dashboard/*`      - Web UI (cookie session required)
//! - `/static/*`         - Static assets
//!
//! # Middleware
//!
//! - **Tracing** - Structured request/response logging
//! - **Rate limiting** - Per-IP token bucket (configurable for proxy deployments)
//! - **Authentication** - Bearer token (API) or cookie session (web)
//! - **Path normalization** - Trailing slash handling

use crate::api;
use crate::api::handlers::{health_handler, redirect_handler};
use crate::api::middleware::{auth, rate_limit, tracing};
use crate::observability::metrics::metrics_handler;
use crate::state::AppState;
use crate::web;
use crate::web::middleware::web_auth;
use axum::routing::get;
use axum::{Router, middleware};
use tower::Layer;
use tower_http::normalize_path::{NormalizePath, NormalizePathLayer};
use tower_http::services::ServeDir;

/// Constructs the application router with all routes and middleware.
///
/// # Arguments
///
/// - `state` - shared application state injected into all handlers
/// - `behind_proxy` - when `true`, rate limiting reads client IP from
///   `X-Forwarded-For` / `X-Real-IP` headers instead of the peer socket address;
///   enable only when the service runs behind a trusted reverse proxy
pub fn app_router(state: AppState, behind_proxy: bool) -> NormalizePath<Router> {
    let api_router = api::routes::protected_routes()
        .route_layer(middleware::from_fn_with_state(state.clone(), auth::layer))
        .layer(rate_limit::secure_layer(behind_proxy));

    let web_protected = web::routes::protected_routes()
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            web_auth::layer,
        ))
        .layer(rate_limit::secure_layer(behind_proxy));

    let web_public = web::routes::public_routes().layer(rate_limit::layer(behind_proxy));

    let web_router = Router::new().merge(web_protected).merge(web_public);

    // The public redirect endpoint is rate-limited per-IP. `/health` is left
    // unthrottled so orchestrator/liveness probes are never rejected with 429.
    let redirect_router = Router::new()
        .route("/{code}", get(redirect_handler))
        .layer(rate_limit::layer(behind_proxy));

    let router = Router::new()
        .merge(redirect_router)
        .route("/health", get(health_handler))
        .route("/metrics", get(metrics_handler))
        .nest("/api", api_router)
        .nest("/dashboard", web_router)
        .nest_service("/static", ServeDir::new("static"))
        .route_layer(middleware::from_fn(
            crate::api::middleware::metrics::track_metrics,
        ))
        .with_state(state)
        .layer(tracing::layer());

    NormalizePathLayer::trim_trailing_slash().layer(router)
}
