use crate::api::handlers::{
    domain_list_handler, health_handler, shorten_handler, stats_handler,
    stats_list_handler,
};
use crate::state::AppState;
use axum::{
    Router,
    routing::{get, post},
};

/// Защищённые маршруты (требуют аутентификацию)
pub fn protected_routes() -> Router<AppState> {
    Router::new()
        .route("/health", get(health_handler))
        .route("/domains", get(domain_list_handler))
        .route("/stats", get(stats_list_handler))
        .route("/stats/{code}", get(stats_handler))
}

/// Публичные маршруты API
pub fn public_routes() -> Router<AppState> {
    Router::new()
        .route("/shorten", post(shorten_handler))
}
