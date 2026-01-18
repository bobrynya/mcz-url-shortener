use crate::state::AppState;
use crate::web::handlers::{dashboard_handler, links_handler, login_handler, stats_handler};
use axum::{Router, routing::get};

/// Защищённые маршруты (требуют аутентификацию)
pub fn protected_routes() -> Router<AppState> {
    Router::new()
        .route("/", get(dashboard_handler))
        .route("/links", get(links_handler))
        .route("/stats/{code}", get(stats_handler))
}

/// Публичные маршруты
pub fn public_routes() -> Router<AppState> {
    Router::new().route("/login", get(login_handler))
}
