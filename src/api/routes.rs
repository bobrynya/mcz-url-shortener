use crate::api::handlers::{
    domain_list_handler, health_handler, redirect_handler, shorten_handler, stats_handler,
    stats_list_handler,
};
use crate::api::middleware::auth;
use crate::api::middleware::rate_limit;
use crate::api::middleware::tracing;
use crate::state::AppState;

use axum::{
    Router, middleware,
    routing::{get, post},
};
use tower::Layer;
use tower_http::normalize_path::{NormalizePath, NormalizePathLayer};

pub fn app_router(state: AppState) -> NormalizePath<Router> {
    // Защищённые маршруты
    let protected_routes = Router::new()
        .route("/health", get(health_handler))
        .route("/domains", get(domain_list_handler))
        .route("/stats", get(stats_list_handler))
        .route("/stats/{code}", get(stats_handler))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth::layer))
        .layer(rate_limit::secure_layer());

    // Публичные маршруты
    let public_routes = Router::new()
        .route("/shorten", post(shorten_handler))
        .route("/{code}", get(redirect_handler))
        .layer(rate_limit::layer());

    let router = Router::new()
        .merge(protected_routes)
        .merge(public_routes)
        .with_state(state)
        .layer(tracing::layer());

    // Убираем trailing slash
    NormalizePathLayer::trim_trailing_slash().layer(router)
}
