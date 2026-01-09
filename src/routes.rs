use crate::{
    handlers::{
        redirect::redirect_by_code, shorten::shorten, stats::stats_by_code, stats_list::stats_list,
    },
    middlewares::access_log::access_log_mw,
    state::AppState,
};

use axum::{
    Router, middleware,
    routing::{get, post},
};

pub fn app_router(state: AppState) -> Router {
    Router::new()
        .route("/shorten", post(shorten))
        .route("/stats/{code}", get(stats_by_code))
        .route("/stats", get(stats_list))
        .route("/{code}", get(redirect_by_code))
        .layer(middleware::from_fn(access_log_mw))
        .with_state(state)
}
