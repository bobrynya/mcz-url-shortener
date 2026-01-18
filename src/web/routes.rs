use crate::state::AppState;
use crate::web::handlers::{dashboard_handler, links_handler, stats_handler};
use axum::{Router, routing::get};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(dashboard_handler))
        .route("/links", get(links_handler))
        .route("/stats/{code}", get(stats_handler))
}
