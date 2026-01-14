use crate::{
    handlers::{
        redirect::redirect_by_code, shorten::shorten, stats::stats_by_code, stats_list::stats_list,
    },
    middleware::access_log::access_log_mw,
    state::AppState,
};

use crate::middleware::auth::auth_mw;
use axum::{
    Router, middleware,
    routing::{get, post},
};

pub fn app_router(state: AppState) -> Router {
    let stats_routes = Router::new()
        .route("/stats", get(stats_list))
        .route("/stats/{code}", get(stats_by_code))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth_mw));

    Router::new()
        .merge(stats_routes)
        .route("/shorten", post(shorten))
        .route("/{code}", get(redirect_by_code))
        .layer(middleware::from_fn(access_log_mw))
        .with_state(state)
}
