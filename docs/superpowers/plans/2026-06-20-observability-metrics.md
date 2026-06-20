# Observability — Prometheus Metrics + Grafana Dashboard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire the existing no-op `metrics` instrumentation to a real Prometheus exporter, add HTTP RED and two business metrics, expose `GET /metrics`, and ship a ready-to-import Grafana dashboard.

**Architecture:** Add `metrics-exporter-prometheus` paired with the already-used `metrics 0.24` facade. A new `src/observability/` module installs a process-global Prometheus recorder once (idempotent via `OnceLock`) and renders the exposition. A `track_metrics` middleware (added via `route_layer`, so `MatchedPath` is populated) records request count and latency under low-cardinality labels. Two business counters live where their branches already are. The Grafana dashboard ships as committed JSON for manual import.

**Tech Stack:** Rust (edition 2024, MSRV 1.96), axum 0.8, tokio 1.x, `metrics` 0.24 facade, `metrics-exporter-prometheus`, axum-test + `#[sqlx::test]` for integration tests.

## Global Constraints

- Rust edition 2024, MSRV **1.96** (pinned via `rust-toolchain.toml`).
- axum **0.8**, tokio **1.x**, `metrics` **0.24** facade (unchanged).
- New dependency `metrics-exporter-prometheus` must build on Rust 1.96 / edition 2024 against `metrics 0.24`.
- No `unwrap()`/`expect()` outside tests, const init, or documented panic-safe contexts.
- Errors via `AppError` + `thiserror`; logging via `tracing::{info,warn,error,debug}`, no `println!` in production code.
- `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` must pass clean.
- Clean Architecture layering preserved; new wiring lives in `src/observability/` and `src/api/middleware/`.
- The `path` label on HTTP metrics comes **only** from `MatchedPath` (route template), never the raw URI — cardinality safety.
- `GET /metrics` is open on the main port, not rate-limited, and not self-instrumented.

## File Structure

**Created:**
- `src/observability/mod.rs` — module root, re-exports.
- `src/observability/metrics.rs` — `install_prometheus_recorder()`, `metrics_handler`.
- `src/api/middleware/metrics.rs` — `track_metrics` middleware + `matched_path_label` helper.
- `dashboards/url-shortener.json` — Grafana dashboard.
- `tests/handler_metrics.rs` — integration tests for the exporter, HTTP RED, and `links_created_total`.

**Modified:**
- `Cargo.toml` — add `metrics-exporter-prometheus`.
- `src/lib.rs` — register `observability` module.
- `src/state.rs` — add `metrics_handle` field; `AppState::new` installs/derives the recorder handle.
- `src/routes.rs` — register `GET /metrics`; add `route_layer(track_metrics)`.
- `src/api/middleware/mod.rs` — register `metrics` middleware module.
- `src/api/handlers/redirect.rs` — emit `cache_requests_total`.
- `src/application/services/link_service.rs` — emit `links_created_total`.
- `tests/handler_redirect.rs` — add a test asserting `cache_requests_total` (reuses its `MockConnectInfoLayer`).
- `README.md`, `CLAUDE.md` — observability documentation.

---

### Task 1: Prometheus exporter + `/metrics` endpoint

**Files:**
- Modify: `Cargo.toml`
- Create: `src/observability/mod.rs`, `src/observability/metrics.rs`
- Modify: `src/lib.rs`, `src/state.rs`, `src/routes.rs`
- Test: `tests/handler_metrics.rs`

**Interfaces:**
- Produces:
  - `url_shortener::observability::metrics::install_prometheus_recorder() -> metrics_exporter_prometheus::PrometheusHandle` (idempotent, process-global).
  - `url_shortener::observability::metrics::metrics_handler` — axum handler reading `State<AppState>`, returns Prometheus exposition text.
  - `AppState` gains a public field `metrics_handle: metrics_exporter_prometheus::PrometheusHandle`.
- Consumes: existing `AppState::new` (sync constructor in `src/state.rs`).

- [ ] **Step 1: Add the exporter dependency**

In `Cargo.toml`, in the `[dependencies]` section directly below the existing `metrics = "0.24"` line, add:

```toml
metrics-exporter-prometheus = { version = "0.16", default-features = false }
```

Run: `cargo build`
Expected: resolves and compiles. If the crate version `0.16` does not depend on `metrics 0.24` (build error about a duplicate `metrics` version), pick the `metrics-exporter-prometheus` minor version whose `metrics` dependency is `0.24` and pin that instead, then re-run `cargo build`.

- [ ] **Step 2: Create the observability module**

Create `src/observability/mod.rs`:

```rust
//! Observability wiring: metrics recorder and exposition.

pub mod metrics;
```

Create `src/observability/metrics.rs`:

```rust
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
```

- [ ] **Step 3: Register the module**

In `src/lib.rs`, add `pub mod observability;` to the module list (next to `pub mod infrastructure;`):

```rust
pub mod infrastructure;
pub mod observability;
pub mod state;
```

- [ ] **Step 4: Carry the handle in `AppState`**

In `src/state.rs`:

Add the import near the other `use crate::...` lines:

```rust
use metrics_exporter_prometheus::PrometheusHandle;
```

Add the field to the `AppState` struct (after the `cookie_secure` field):

```rust
    /// Whether the dashboard auth cookie should be marked `Secure`.
    pub cookie_secure: bool,

    /// Prometheus exposition handle, rendered by `GET /metrics`.
    pub metrics_handle: PrometheusHandle,
```

In `AppState::new`, derive the handle (no new constructor parameter) just before the returned `Self { ... }` and add it to the struct literal:

```rust
        let metrics_handle = crate::observability::metrics::install_prometheus_recorder();

        Self {
            link_service,
            stats_service,
            auth_service,
            domain_service,
            cache,
            click_publisher,
            kafka,
            clickhouse,
            cookie_secure,
            metrics_handle,
        }
```

- [ ] **Step 5: Register the `/metrics` route**

In `src/routes.rs`, add the import next to the other handler imports:

```rust
use crate::observability::metrics::metrics_handler;
```

Add the route to the top-level `router` builder (after the `/health` route, before `.nest("/api", ...)`):

```rust
        .route("/health", get(health_handler))
        .route("/metrics", get(metrics_handler))
        .nest("/api", api_router)
```

Run: `cargo build`
Expected: compiles clean.

- [ ] **Step 6: Write the exposition integration test**

Create `tests/handler_metrics.rs`:

```rust
mod common;

use axum::Router;
use axum::routing::get;
use axum_test::TestServer;
use sqlx::PgPool;
use url_shortener::observability::metrics::metrics_handler;

#[sqlx::test]
async fn metrics_endpoint_returns_prometheus_exposition(pool: PgPool) {
    let state = common::create_test_state(pool);
    let app = Router::new()
        .route("/metrics", get(metrics_handler))
        .with_state(state);
    let server = TestServer::new(app).unwrap();

    let response = server.get("/metrics").await;

    response.assert_status_ok();
    let content_type = response.header("content-type");
    let content_type = content_type.to_str().unwrap();
    assert!(
        content_type.starts_with("text/plain"),
        "unexpected content-type: {content_type}"
    );
}
```

- [ ] **Step 7: Run the test**

Run: `cargo test --test handler_metrics`
Expected: `metrics_endpoint_returns_prometheus_exposition` passes.

- [ ] **Step 8: Lint and format**

Run: `cargo fmt && cargo clippy --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml Cargo.lock src/observability src/lib.rs src/state.rs src/routes.rs tests/handler_metrics.rs
git commit -m "feat(observability): install Prometheus recorder and expose /metrics"
```

---

### Task 2: HTTP RED middleware

**Files:**
- Create: `src/api/middleware/metrics.rs`
- Modify: `src/api/middleware/mod.rs`, `src/routes.rs`
- Test: `src/api/middleware/metrics.rs` (unit), `tests/handler_metrics.rs` (integration)

**Interfaces:**
- Consumes: `metrics_handler` and `install_prometheus_recorder` from Task 1.
- Produces:
  - `url_shortener::api::middleware::metrics::track_metrics(req: axum::extract::Request, next: axum::middleware::Next) -> axum::response::Response`.
  - `url_shortener::api::middleware::metrics::matched_path_label(matched: Option<&axum::extract::MatchedPath>) -> String`.
  - Emits `http_requests_total{method, path, status}` (counter) and `http_request_duration_seconds{method, path, status}` (histogram, seconds).

- [ ] **Step 1: Write the middleware and its label helper**

Create `src/api/middleware/metrics.rs`:

```rust
//! HTTP RED metrics middleware: request count + latency, low-cardinality labels.

use std::time::Instant;

use axum::extract::{MatchedPath, Request};
use axum::middleware::Next;
use axum::response::Response;

/// Returns the low-cardinality route-template label for a request.
///
/// Uses the matched route pattern (e.g. `/{code}`, `/api/links/{code}`), never
/// the raw URI — otherwise every short code becomes its own time series. Requests
/// that reached this layer without a matched path are labelled `"<unmatched>"`.
pub fn matched_path_label(matched: Option<&MatchedPath>) -> String {
    matched
        .map(|m| m.as_str().to_owned())
        .unwrap_or_else(|| "<unmatched>".to_owned())
}

/// Records `http_requests_total` and `http_request_duration_seconds` per request.
///
/// `/metrics` is skipped so Prometheus scrapes don't inflate the HTTP metrics.
pub async fn track_metrics(req: Request, next: Next) -> Response {
    let path = matched_path_label(req.extensions().get::<MatchedPath>());

    if path == "/metrics" {
        return next.run(req).await;
    }

    let method = req.method().as_str().to_owned();
    let start = Instant::now();
    let response = next.run(req).await;
    let elapsed = start.elapsed().as_secs_f64();
    let status = response.status().as_u16().to_string();

    metrics::counter!(
        "http_requests_total",
        "method" => method.clone(),
        "path" => path.clone(),
        "status" => status.clone(),
    )
    .increment(1);
    metrics::histogram!(
        "http_request_duration_seconds",
        "method" => method,
        "path" => path,
        "status" => status,
    )
    .record(elapsed);

    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_falls_back_when_no_matched_path() {
        // `MatchedPath` has no public constructor; the populated case is covered
        // by the integration test asserting `path="/{code}"`. Here we pin the
        // fallback used for unmatched requests.
        assert_eq!(matched_path_label(None), "<unmatched>");
    }
}
```

- [ ] **Step 2: Run the unit test (verify it passes)**

Run: `cargo test --lib middleware::metrics`
Expected: `label_falls_back_when_no_matched_path` passes.

- [ ] **Step 3: Register the middleware module**

In `src/api/middleware/mod.rs`, add to the module list:

```rust
pub mod auth;
pub mod metrics;
pub mod rate_limit;
pub mod tracing;
```

- [ ] **Step 4: Apply the middleware in the router**

In `src/routes.rs`, add the layer to the top-level `router`. It must be added with `route_layer` (not `layer`) so `MatchedPath` is populated when the middleware runs. Place it immediately after `.nest_service("/static", ...)` and before `.with_state(state)`:

```rust
        .nest_service("/static", ServeDir::new("static"))
        .route_layer(middleware::from_fn(
            crate::api::middleware::metrics::track_metrics,
        ))
        .with_state(state)
        .layer(tracing::layer());
```

`middleware` is already imported in `routes.rs` (`use axum::{Router, middleware};`).

Run: `cargo build`
Expected: compiles clean.

- [ ] **Step 5: Write the integration test for templating + self-exclusion**

Append to `tests/handler_metrics.rs`:

```rust
#[sqlx::test]
async fn http_metrics_use_route_template_and_exclude_self(pool: PgPool) {
    use url_shortener::api::middleware::metrics::track_metrics;

    let state = common::create_test_state(pool);
    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/{code}", get(|| async { "ok" }))
        .route("/metrics", get(metrics_handler))
        .route_layer(axum::middleware::from_fn(track_metrics))
        .with_state(state);
    let server = TestServer::new(app).unwrap();

    server.get("/health").await;
    server.get("/abc123XYZ").await;
    let body = server.get("/metrics").await.text();

    assert!(
        body.contains("http_requests_total"),
        "missing http_requests_total in:\n{body}"
    );
    // Route template label, not the raw code:
    assert!(
        body.contains(r#"path="/{code}""#),
        "missing templated path label in:\n{body}"
    );
    assert!(
        !body.contains("abc123XYZ"),
        "raw short code leaked into a label in:\n{body}"
    );
    // /metrics must not instrument itself:
    assert!(
        !body.contains(r#"path="/metrics""#),
        "/metrics was self-instrumented in:\n{body}"
    );
}
```

- [ ] **Step 6: Run the integration tests**

Run: `cargo test --test handler_metrics`
Expected: both tests pass.

- [ ] **Step 7: Lint and format**

Run: `cargo fmt && cargo clippy --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 8: Commit**

```bash
git add src/api/middleware/metrics.rs src/api/middleware/mod.rs src/routes.rs tests/handler_metrics.rs
git commit -m "feat(observability): add HTTP RED metrics middleware"
```

---

### Task 3: Business metrics (cache hit-rate + links created)

**Files:**
- Modify: `src/api/handlers/redirect.rs`, `src/application/services/link_service.rs`
- Test: `tests/handler_redirect.rs` (cache), `tests/handler_metrics.rs` (links created)

**Interfaces:**
- Consumes: `install_prometheus_recorder` from Task 1; existing `LinkService::create_short_link`, `redirect_handler`.
- Produces: `cache_requests_total{result="hit"|"miss"|"error"}` (counter), `links_created_total` (counter).

- [ ] **Step 1: Emit `cache_requests_total` in the redirect handler**

In `src/api/handlers/redirect.rs::redirect_handler`, the `match state.cache.get_url(&cache_key).await { ... }` has three arms (`Ok(Some)`, `Ok(None)`, `Err`). Add one counter increment as the first line of each arm:

In the `Ok(Some(cached_value))` arm, as the first statement:

```rust
        Ok(Some(cached_value)) => {
            metrics::counter!("cache_requests_total", "result" => "hit").increment(1);
            debug!("Cache HIT for {}", cache_key);
```

In the `Ok(None)` arm, as the first statement:

```rust
        Ok(None) => {
            metrics::counter!("cache_requests_total", "result" => "miss").increment(1);
            debug!("Cache MISS for {}", cache_key);
```

In the `Err(e)` arm, as the first statement:

```rust
        Err(e) => {
            metrics::counter!("cache_requests_total", "result" => "error").increment(1);
            error!("Cache error: {}", e);
```

- [ ] **Step 2: Emit `links_created_total` in the link service**

In `src/application/services/link_service.rs::create_short_link_for_domain`, the function currently ends with:

```rust
        self.link_repository.create(new_link).await
    }
```

Replace that final expression so the counter increments only on a genuinely created link (the earlier dedup branch returns before reaching here):

```rust
        let created = self.link_repository.create(new_link).await?;
        metrics::counter!("links_created_total").increment(1);
        Ok(created)
    }
```

Run: `cargo build`
Expected: compiles clean.

- [ ] **Step 3: Write the `links_created_total` integration test**

Append to `tests/handler_metrics.rs`:

```rust
#[sqlx::test]
async fn links_created_total_increments_on_create(pool: PgPool) {
    use url_shortener::observability::metrics::install_prometheus_recorder;

    let handle = install_prometheus_recorder();
    let state = common::create_test_state(pool);

    state
        .link_service
        .create_short_link("https://example.com/page".to_owned(), None, None, false)
        .await
        .unwrap();

    let body = handle.render();
    assert!(
        body.contains("links_created_total"),
        "missing links_created_total in:\n{body}"
    );
}
```

- [ ] **Step 4: Write the `cache_requests_total` integration test**

Append to `tests/handler_redirect.rs` (this file already defines `MockConnectInfoLayer` and imports `redirect_handler`, `RecordingClickPublisher`, `Router`, `get`, `TestServer`, `SocketAddr`, `Arc`). Add at the end of the file:

```rust
#[sqlx::test]
async fn redirect_emits_cache_miss_metric(pool: PgPool) {
    use url_shortener::observability::metrics::install_prometheus_recorder;

    let handle = install_prometheus_recorder();
    let domain_id = common::get_default_domain(&pool).await;
    common::create_test_link(&pool, "cmiss", "https://example.com/cmiss", domain_id).await;

    let state = common::create_test_state(pool.clone());
    let app = Router::new()
        .route("/{code}", get(redirect_handler))
        .layer(MockConnectInfoLayer)
        .with_state(state);
    let server = TestServer::new(app).unwrap();

    // NullCache always misses, so the first redirect emits result="miss".
    server.get("/cmiss").add_header("Host", "s.example.com").await;

    let body = handle.render();
    assert!(
        body.contains("cache_requests_total"),
        "missing cache_requests_total in:\n{body}"
    );
    assert!(
        body.contains(r#"result="miss""#),
        "missing result=\"miss\" label in:\n{body}"
    );
}
```

If the existing `Host` header value used by other tests in this file differs from `s.example.com` (the seeded default domain), match whatever the other passing redirect tests in this file use.

- [ ] **Step 5: Run the tests**

Run: `cargo test --test handler_metrics --test handler_redirect`
Expected: the two new tests pass, and the existing redirect tests still pass.

- [ ] **Step 6: Lint and format**

Run: `cargo fmt && cargo clippy --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 7: Commit**

```bash
git add src/api/handlers/redirect.rs src/application/services/link_service.rs tests/handler_metrics.rs tests/handler_redirect.rs
git commit -m "feat(observability): add cache hit-rate and links-created metrics"
```

---

### Task 4: Grafana dashboard JSON

**Files:**
- Create: `dashboards/url-shortener.json`
- Test: `tests/handler_metrics.rs` (JSON validity)

**Interfaces:**
- Consumes: metric names from Tasks 1–3 and the existing `database_errors_total`, `click_publish_total`, `click_consumer_received_total`, `click_consumer_inserted_total`, `click_consumer_insert_failed_total`, `click_publish_dropped_total`, `click_consumer_batch_size`.
- Produces: an importable Grafana dashboard with a `${DS_PROMETHEUS}` datasource variable.

- [ ] **Step 1: Create the dashboard JSON**

Create `dashboards/url-shortener.json`. Use a `__inputs` datasource of type `prometheus` named `DS_PROMETHEUS`, and reference `${DS_PROMETHEUS}` in every panel so the dashboard imports into any Grafana without a hard-coded datasource UID. Include these panels:

```json
{
  "__inputs": [
    {
      "name": "DS_PROMETHEUS",
      "label": "Prometheus",
      "description": "",
      "type": "datasource",
      "pluginId": "prometheus",
      "pluginName": "Prometheus"
    }
  ],
  "__requires": [],
  "annotations": { "list": [] },
  "editable": true,
  "graphTooltip": 0,
  "schemaVersion": 39,
  "tags": ["url-shortener"],
  "templating": { "list": [] },
  "time": { "from": "now-6h", "to": "now" },
  "timezone": "",
  "title": "URL Shortener — Service Metrics",
  "uid": "url-shortener-metrics",
  "version": 1,
  "panels": [
    {
      "id": 1,
      "title": "Request rate by status",
      "type": "timeseries",
      "datasource": { "type": "prometheus", "uid": "${DS_PROMETHEUS}" },
      "gridPos": { "h": 8, "w": 12, "x": 0, "y": 0 },
      "targets": [
        {
          "expr": "sum by (status) (rate(http_requests_total[5m]))",
          "legendFormat": "{{status}}",
          "refId": "A"
        }
      ]
    },
    {
      "id": 2,
      "title": "Request latency (p50/p95/p99)",
      "type": "timeseries",
      "datasource": { "type": "prometheus", "uid": "${DS_PROMETHEUS}" },
      "gridPos": { "h": 8, "w": 12, "x": 12, "y": 0 },
      "targets": [
        {
          "expr": "histogram_quantile(0.50, sum by (le) (rate(http_request_duration_seconds_bucket[5m])))",
          "legendFormat": "p50",
          "refId": "A"
        },
        {
          "expr": "histogram_quantile(0.95, sum by (le) (rate(http_request_duration_seconds_bucket[5m])))",
          "legendFormat": "p95",
          "refId": "B"
        },
        {
          "expr": "histogram_quantile(0.99, sum by (le) (rate(http_request_duration_seconds_bucket[5m])))",
          "legendFormat": "p99",
          "refId": "C"
        }
      ]
    },
    {
      "id": 3,
      "title": "4xx / 5xx error ratio",
      "type": "timeseries",
      "datasource": { "type": "prometheus", "uid": "${DS_PROMETHEUS}" },
      "gridPos": { "h": 8, "w": 12, "x": 0, "y": 8 },
      "targets": [
        {
          "expr": "sum(rate(http_requests_total{status=~\"5..\"}[5m])) / sum(rate(http_requests_total[5m]))",
          "legendFormat": "5xx ratio",
          "refId": "A"
        },
        {
          "expr": "sum(rate(http_requests_total{status=~\"4..\"}[5m])) / sum(rate(http_requests_total[5m]))",
          "legendFormat": "4xx ratio",
          "refId": "B"
        }
      ]
    },
    {
      "id": 4,
      "title": "Cache hit-rate",
      "type": "timeseries",
      "datasource": { "type": "prometheus", "uid": "${DS_PROMETHEUS}" },
      "gridPos": { "h": 8, "w": 12, "x": 12, "y": 8 },
      "targets": [
        {
          "expr": "sum(rate(cache_requests_total{result=\"hit\"}[5m])) / sum(rate(cache_requests_total[5m]))",
          "legendFormat": "hit-rate",
          "refId": "A"
        }
      ]
    },
    {
      "id": 5,
      "title": "Redirects by status",
      "type": "timeseries",
      "datasource": { "type": "prometheus", "uid": "${DS_PROMETHEUS}" },
      "gridPos": { "h": 8, "w": 12, "x": 0, "y": 16 },
      "targets": [
        {
          "expr": "sum by (status) (rate(http_requests_total{path=\"/{code}\"}[5m]))",
          "legendFormat": "{{status}}",
          "refId": "A"
        }
      ]
    },
    {
      "id": 6,
      "title": "Links created rate",
      "type": "timeseries",
      "datasource": { "type": "prometheus", "uid": "${DS_PROMETHEUS}" },
      "gridPos": { "h": 8, "w": 12, "x": 12, "y": 16 },
      "targets": [
        {
          "expr": "rate(links_created_total[5m])",
          "legendFormat": "links/s",
          "refId": "A"
        }
      ]
    },
    {
      "id": 7,
      "title": "Database errors by type",
      "type": "timeseries",
      "datasource": { "type": "prometheus", "uid": "${DS_PROMETHEUS}" },
      "gridPos": { "h": 8, "w": 12, "x": 0, "y": 24 },
      "targets": [
        {
          "expr": "sum by (type) (rate(database_errors_total[5m]))",
          "legendFormat": "{{type}}",
          "refId": "A"
        }
      ]
    },
    {
      "id": 8,
      "title": "Click pipeline",
      "type": "timeseries",
      "datasource": { "type": "prometheus", "uid": "${DS_PROMETHEUS}" },
      "gridPos": { "h": 8, "w": 12, "x": 12, "y": 24 },
      "targets": [
        { "expr": "rate(click_publish_total[5m])", "legendFormat": "published", "refId": "A" },
        { "expr": "rate(click_consumer_received_total[5m])", "legendFormat": "received", "refId": "B" },
        { "expr": "rate(click_consumer_inserted_total[5m])", "legendFormat": "inserted", "refId": "C" },
        { "expr": "rate(click_consumer_insert_failed_total[5m])", "legendFormat": "insert_failed", "refId": "D" },
        { "expr": "sum by (reason) (rate(click_publish_dropped_total[5m]))", "legendFormat": "dropped {{reason}}", "refId": "E" }
      ]
    },
    {
      "id": 9,
      "title": "Click batch size (p95)",
      "type": "timeseries",
      "datasource": { "type": "prometheus", "uid": "${DS_PROMETHEUS}" },
      "gridPos": { "h": 8, "w": 12, "x": 0, "y": 32 },
      "targets": [
        {
          "expr": "histogram_quantile(0.95, sum by (le) (rate(click_consumer_batch_size_bucket[5m])))",
          "legendFormat": "p95 batch size",
          "refId": "A"
        }
      ]
    }
  ]
}
```

- [ ] **Step 2: Write the JSON-validity test**

Append to `tests/handler_metrics.rs`:

```rust
#[test]
fn grafana_dashboard_json_is_valid() {
    let raw = std::fs::read_to_string("dashboards/url-shortener.json")
        .expect("dashboards/url-shortener.json must exist");
    let value: serde_json::Value =
        serde_json::from_str(&raw).expect("dashboard must be valid JSON");
    assert!(value.get("panels").and_then(|p| p.as_array()).is_some());
    assert!(
        raw.contains("${DS_PROMETHEUS}"),
        "dashboard must use the DS_PROMETHEUS datasource variable"
    );
}
```

- [ ] **Step 3: Run the test**

Run: `cargo test --test handler_metrics grafana_dashboard_json_is_valid`
Expected: passes. (Working directory for `cargo test` is the crate root, so the relative path resolves.)

- [ ] **Step 4: Commit**

```bash
git add dashboards/url-shortener.json tests/handler_metrics.rs
git commit -m "feat(observability): add Grafana dashboard JSON"
```

---

### Task 5: Documentation

**Files:**
- Modify: `README.md`, `CLAUDE.md`

**Interfaces:**
- Consumes: all metric names and the `/metrics` endpoint from Tasks 1–4.

- [ ] **Step 1: Document metrics in README**

In `README.md`, add a new top-level section "## Observability" (place it after the existing configuration/run sections — pick the location that reads naturally). Content:

````markdown
## Observability

The service exposes Prometheus metrics at `GET /metrics` (text exposition format,
`text/plain; version=0.0.4`).

### Security

`/metrics` is **open on the main service port**. It exposes internal operational
metrics, so production deployments **must** block the `/metrics` path at the load
balancer / ingress / firewall. Relocating it to a separate admin port or putting
it behind authentication is intentionally out of scope for now.

### Metrics

| Metric | Type | Labels | Meaning |
|--------|------|--------|---------|
| `http_requests_total` | counter | `method`, `path`, `status` | HTTP requests; `path` is the route template (e.g. `/{code}`), never the raw path |
| `http_request_duration_seconds` | histogram | `method`, `path`, `status` | Request latency in seconds |
| `cache_requests_total` | counter | `result` (`hit`/`miss`/`error`) | Redis cache outcomes on redirect |
| `links_created_total` | counter | — | Links actually created (excludes deduplicated hits) |
| `database_errors_total` | counter | `type` | Database errors by category |
| `click_publish_total` | counter | — | Click events published to Kafka |
| `click_publish_dropped_total` | counter | `reason` | Click events dropped before publish |
| `click_consumer_received_total` | counter | — | Click events received by the consumer |
| `click_consumer_inserted_total` | counter | — | Click events inserted into ClickHouse |
| `click_consumer_insert_failed_total` | counter | — | Failed ClickHouse inserts |
| `click_consumer_batch_size` | histogram | — | Consumer batch sizes |

### Grafana dashboard

Import `dashboards/url-shortener.json` into Grafana (Dashboards → Import). On
import, select your Prometheus data source for the `DS_PROMETHEUS` variable.
````

- [ ] **Step 2: Document the convention in CLAUDE.md**

In `CLAUDE.md`, under the "Code Conventions" section, add a bullet:

```markdown
- **Metrics**: emit via the `metrics` facade (`metrics::counter!`/`histogram!`). The
  Prometheus recorder + `/metrics` endpoint live in `src/observability/metrics.rs`;
  HTTP metrics are recorded by `src/api/middleware/metrics.rs`. The `path` label
  must always come from `MatchedPath` (route template), never the raw URI.
```

- [ ] **Step 3: Verify formatting of docs**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: no changes, no warnings (docs-only task; this just confirms nothing else drifted).

- [ ] **Step 4: Commit**

```bash
git add README.md CLAUDE.md
git commit -m "docs(observability): document /metrics endpoint and metric catalog"
```

---

## Final Verification

After all tasks:

- [ ] Run: `cargo fmt --check` — clean.
- [ ] Run: `cargo clippy --all-targets -- -D warnings` — clean.
- [ ] Run: `cargo test` — all green.
- [ ] Manual smoke (optional): `cargo run`, then `curl -s localhost:<port>/metrics | head` shows exposition; after a few requests, `http_requests_total` appears with templated `path` labels.
