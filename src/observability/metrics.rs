//! Prometheus metrics recorder installation and the `/metrics` exposition handler.
//!
//! The `metrics` facade is used throughout the codebase. This module installs a
//! process-global Prometheus recorder (once) so those emissions are captured, and
//! renders them for Prometheus to scrape at `GET /metrics`.

use std::sync::OnceLock;

use axum::extract::State;
use axum::http::header;
use axum::response::IntoResponse;
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};

use crate::state::AppState;

/// Explicit latency buckets (seconds) for `http_request_duration_seconds`.
///
/// Rendered as Prometheus histogram buckets rather than summary quantiles, so
/// `histogram_quantile()` works in Grafana/PromQL.
const HTTP_LATENCY_BUCKETS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

/// Process-wide handle. The global recorder can only be installed once per
/// process; this guard makes repeated calls (production startup and many parallel
/// tests) return a clone of the same handle instead of panicking on re-install.
static HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// Installs the global Prometheus recorder (idempotent) and returns its handle.
///
/// First call builds and installs the recorder; later calls return a clone of the
/// already-installed handle.
pub fn install_prometheus_recorder() -> PrometheusHandle {
    HANDLE
        .get_or_init(|| {
            // Panic-safe: the only failure modes are a malformed bucket matcher
            // (a compile-time-constant programming error) or a second global
            // recorder install (prevented by this `OnceLock`). Both are startup
            // invariants, mirroring `server.rs`'s `migrate(...).expect(...)`.
            let builder = PrometheusBuilder::new()
                .set_buckets_for_metric(
                    Matcher::Full("http_request_duration_seconds".to_owned()),
                    HTTP_LATENCY_BUCKETS,
                )
                .expect("valid latency bucket configuration");
            builder
                .install_recorder()
                .expect("install global Prometheus recorder")
        })
        .clone()
}

/// Renders the current metrics in Prometheus text exposition format.
///
/// `GET /metrics`. Always returns 200.
pub async fn metrics_handler(State(state): State<AppState>) -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        state.metrics_handle.render(),
    )
}
