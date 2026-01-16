use crate::api::handlers::{redirect_handler, shorten_handler, stats_handler, stats_list_handler};
use crate::api::middleware::access_log::access_log_mw;
use crate::api::middleware::auth::auth_mw;
use crate::state::AppState;

use axum::{
    Router, middleware,
    routing::{get, post},
};

pub fn app_router(state: AppState) -> Router {
    // Защищённые маршруты
    let protected_routes = Router::new()
        .route("/stats", get(stats_list_handler))
        .route("/stats/{code}", get(stats_handler))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth_mw));

    // Публичные маршруты
    let public_routes = Router::new()
        .route("/shorten", post(shorten_handler))
        .route("/{code}", get(redirect_handler));

    Router::new()
        .merge(protected_routes)
        .merge(public_routes)
        .layer(middleware::from_fn(access_log_mw))
        .with_state(state)
}
