# Phase 3a: Observability — Prometheus Metrics + Grafana Dashboard — Design

> **Phase:** 3 of the post-Phase-1 roadmap, scoped to **metrics only**.
> Phase 1 = clicks on Kafka + ClickHouse; Phase 2 = bulk deactivation +
> `domain_id` unification. The roadmap's Phase 3 names three independent
> observability subsystems — metrics/Grafana, tracing/APM, errors/Sentry.
> This spec covers **only metrics → Prometheus → Grafana**. Distributed
> tracing (OpenTelemetry/APM) and error tracking (Sentry) remain separate,
> future specs.

## Goal

Make the URL shortener observable through Prometheus-format metrics: wire the
existing (currently no-op) `metrics` instrumentation to a real recorder, add
HTTP RED metrics and two business metrics, expose a `/metrics` endpoint, and
ship a ready-to-import Grafana dashboard.

## Background

The codebase already calls the `metrics` facade (`metrics = "0.24"`) throughout:
database error counters in `src/error.rs`, Kafka producer counters in
`src/infrastructure/messaging/kafka_producer.rs`, and click-consumer
counters/histograms in `src/infrastructure/messaging/click_consumer.rs`.
**No recorder or exporter is installed**, so every one of these macro calls
currently routes to the global no-op recorder and is discarded. There is no
`/metrics` endpoint. Logging via `tracing` (with a JSON mode) is already set up;
this spec does not touch logging or tracing-as-spans.

This phase finishes the half-built metrics pipeline rather than introducing a
new instrumentation system.

## Global Constraints

- Rust edition 2024, MSRV **1.96** (pinned via `rust-toolchain.toml`).
- axum **0.8**, tokio **1.x**, `metrics` **0.24** facade (unchanged).
- New exporter dependency: `metrics-exporter-prometheus`, version compatible
  with `metrics 0.24` (the plan pins the exact version and verifies it builds
  on MSRV 1.96 / edition 2024).
- No `unwrap()`/`expect()` outside tests, const init, or documented
  panic-safe contexts (`src/error.rs` conventions).
- Errors via `AppError` + `thiserror`; logging via `tracing::{info,warn,error,debug}`,
  no `println!` in production code.
- `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` must pass clean.
- Clean Architecture layering preserved: `domain → application → infrastructure → api/web`.
  New observability wiring lives in `src/observability/` and `src/api/middleware/`.

## Scope

**In scope:**
- Prometheus exporter + global recorder install.
- `GET /metrics` endpoint (open, on the main port).
- HTTP RED middleware (request count + latency histogram, low-cardinality labels).
- Two business metrics: Redis cache hit-rate and links-created count.
- Activation of all existing `metrics` instrumentation (no code change to those call sites).
- A committed Grafana dashboard JSON for manual import.
- README + CLAUDE.md documentation.

**Out of scope (future specs):**
- Distributed tracing / OpenTelemetry / Elastic APM.
- Error tracking / Sentry.
- Bundling Prometheus + Grafana into `docker-compose` (dashboard JSON only).
- An env toggle for the exporter (always-on by decision).
- Authenticating or relocating `/metrics` off the main port (perimeter is the
  deployment's responsibility; see Security).

## Architecture

### 1. Exporter and `/metrics` endpoint

New module `src/observability/metrics.rs` (and `src/observability/mod.rs`,
registered in `src/lib.rs`):

- `install_prometheus_recorder() -> PrometheusHandle`
  - Builds a `PrometheusBuilder`.
  - Configures **explicit latency buckets** for the `http_request_duration_seconds`
    histogram (e.g. `[0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]`),
    rendered as Prometheus histogram buckets — **not** summary quantiles.
  - Installs the recorder as the global recorder and returns the `PrometheusHandle`.
  - **Idempotent** via a process-wide `OnceLock<PrometheusHandle>`: the first call
    installs the recorder; subsequent calls return a clone of the same handle.
    This lets parallel integration tests build the router safely (the global
    recorder can only be installed once per process).
- `server::run` calls `install_prometheus_recorder()`, obtains the handle, and
  passes it into `app_router`; the handle is stored in `AppState` (consistent
  with how every other dependency reaches handlers) and the `/metrics` handler
  reads it via `State<AppState>`.
- `metrics_handler` renders `handle.render()` and returns it with content type
  `text/plain; version=0.0.4; charset=utf-8`.
- `GET /metrics` is registered at the top level in `src/routes.rs`:
  **open** (no auth), **not rate-limited**, and **excluded from the HTTP RED
  middleware** so Prometheus scrapes don't inflate the request metrics.

### 2. HTTP RED middleware

New middleware `src/api/middleware/metrics.rs`, function `track_metrics`
(via `axum::middleware::from_fn`), registered in `src/routes.rs::app_router`
as a layer alongside the existing `tracing::layer()`, applied so that
`/metrics` is not covered by it.

Per request:
- Record `Instant::now()` before `next.run(req)`; compute elapsed seconds after.
- Read **`axum::extract::MatchedPath`** from request extensions for the `path`
  label — the route **template** (`/{code}`, `/api/links/{code}`), never the raw
  path. This bounds cardinality (otherwise every short code becomes its own
  series). When no `MatchedPath` is present (unmatched 404), the label is
  `"<unmatched>"`.
- `method` label = HTTP method; `status` label = response status code.

Emits:
- `http_requests_total{method, path, status}` — counter.
- `http_request_duration_seconds{method, path, status}` — histogram (seconds;
  buckets from §1).

`/static/*` requests are instrumented; their `path` label collapses to the
`nest_service` mount template, so cardinality stays bounded.

### 3. Business metrics

Two metrics that HTTP RED cannot express, placed where the existing branches
already are:

- `cache_requests_total{result="hit"|"miss"|"error"}` — counter in
  `src/api/handlers/redirect.rs::redirect_handler`, one increment per branch of
  the existing `state.cache.get_url(...)` match (`Ok(Some)` → `hit`,
  `Ok(None)` → `miss`, `Err` → `error`). Emitted in the handler; the
  `CacheService` trait is not modified. Gives Redis cache hit-rate.
- `links_created_total` — counter incremented on successful link creation in
  `src/application/services/link_service.rs`. Counts links actually created,
  including per-item within a batch — which the `POST /api/shorten` request
  counter from HTTP RED does not show.

**Deliberately excluded** (YAGNI): a separate `redirects_total{type=...}`.
Redirect success / 404 / 410 and permanent vs temporary are already visible via
`http_requests_total{path="/{code}", status}` — a dedicated counter would
duplicate it.

**Existing instrumentation** (DB errors in `src/error.rs`, Kafka producer,
click consumer) is **not modified** — it begins emitting as soon as the recorder
is installed.

### 4. Grafana dashboard and documentation

- `dashboards/url-shortener.json` (new directory) — a Grafana dashboard exported
  for manual import. Its datasource is a dashboard variable `${DS_PROMETHEUS}`
  (type `datasource`, query `prometheus`) so it imports into any instance
  without a hard-coded datasource UID.
- Panels (PromQL over the metrics above):
  - **HTTP**: request rate `sum by (status) (rate(http_requests_total[5m]))`;
    latency p50/p95/p99 via
    `histogram_quantile(0.95, sum by (le) (rate(http_request_duration_seconds_bucket[5m])))`;
    4xx/5xx error ratio.
  - **Redirect / cache**: cache hit-rate
    `sum(rate(cache_requests_total{result="hit"}[5m])) / sum(rate(cache_requests_total[5m]))`;
    redirect status breakdown from `http_requests_total{path="/{code}"}`.
  - **Business**: `rate(links_created_total[5m])`.
  - **Database**: `database_errors_total` broken down by `type` label.
  - **Click pipeline**: `click_publish_total`, `click_consumer_received_total`,
    `click_consumer_inserted_total`, `click_consumer_insert_failed_total`,
    `click_publish_dropped_total`; `click_consumer_batch_size` histogram.
- `README.md` — new "Observability / Metrics" section: the `/metrics` endpoint,
  its format, the metric catalog with labels, dashboard import instructions, and
  the production note on restricting `/metrics` at the edge.
- `CLAUDE.md` — short note on the `src/observability/` module and the
  instrumentation convention (`metrics::*`; the `path` label comes only from
  `MatchedPath`).

## Data Flow

```
request → [track_metrics middleware] → handler
              │                            │
              │ http_requests_total        │ cache_requests_total (redirect)
              │ http_request_duration_*    │ links_created_total (shorten)
              ▼                            ▼
        global Prometheus recorder (metrics facade)
              ▲                            ▲
   existing: database_errors_total   click_* / kafka_*
              │
   Prometheus scrape ──GET /metrics──→ handle.render() → exposition text
              │
        Grafana (imports dashboards/url-shortener.json) → panels
```

## Error Handling

- The `/metrics` handler cannot fail in normal operation: `handle.render()`
  returns a `String`. No `AppError` path needed; it always returns 200.
- If `install_prometheus_recorder()` were called when a recorder is already
  installed by something else, the `OnceLock` guard ensures we never attempt a
  second global install within this process; the first installed handle wins.
- Metric emission is infallible (facade macros never error); a missing recorder
  degrades to no-op, never to a request failure.

## Security

`/metrics` is **open on the main port** — a deliberate decision for simplicity.
The endpoint leaks internal operational metrics, so production deployments
**must** restrict the `/metrics` path at the load balancer / firewall / ingress.
This is documented in README. Relocating `/metrics` to a separate admin port or
putting it behind auth is explicitly deferred (not in this spec).

## Testing

Global recorder is process-wide; `install_prometheus_recorder()` is idempotent
via `OnceLock`, so parallel integration tests can each build the router.

- **Integration tests** (`tests/`, axum-test `TestServer`):
  - `GET /metrics` → 200, `content-type` is `text/plain`, body is
    Prometheus exposition (contains `# HELP` / `# TYPE`).
  - After a request to a matched route (e.g. `/health`), `/metrics` contains
    `http_requests_total`; after a request to `/{code}`, the series carries
    `path="/{code}"` (template), confirming the anti-cardinality label.
  - `/metrics` does not instrument itself (no `http_requests_total` series with
    `path="/metrics"` after a scrape).
- **Unit tests:**
  - `track_metrics`: correct `path` label from `MatchedPath`; `"<unmatched>"`
    fallback when no match.
  - `cache_requests_total`: hit/miss/error branches via a mock `CacheService`.
  - `links_created_total`: increment on successful create in `LinkService`
    (mock repository).
- **Build / quality gates:** `cargo fmt --check`; `cargo clippy --all-targets
  -- -D warnings`; full `cargo test` green; `dashboards/url-shortener.json`
  parses as valid JSON (a small test or a `jq` check).

## Files Created / Modified

**Created:**
- `src/observability/mod.rs`, `src/observability/metrics.rs` — recorder install + `/metrics` handler.
- `src/api/middleware/metrics.rs` — HTTP RED middleware.
- `dashboards/url-shortener.json` — Grafana dashboard.

**Modified:**
- `Cargo.toml` — add `metrics-exporter-prometheus`.
- `src/lib.rs` — register `observability` module.
- `src/server.rs` — install recorder, pass handle into router.
- `src/routes.rs` — register `GET /metrics`, layer `track_metrics`, exclude `/metrics`.
- `src/api/middleware/mod.rs` — register `metrics` middleware module.
- `src/state.rs` — carry the `PrometheusHandle` so `/metrics` reads it via `State<AppState>`.
- `src/api/handlers/redirect.rs` — `cache_requests_total`.
- `src/application/services/link_service.rs` — `links_created_total`.
- `README.md`, `CLAUDE.md` — observability documentation.

## Risks

- **Exporter / MSRV compatibility** — `metrics-exporter-prometheus` must build on
  Rust 1.96 / edition 2024 with `metrics 0.24`. Mitigation: pin and verify the
  version at the first implementation step.
- **Label cardinality** — the `path` label must come from `MatchedPath`, never
  the raw URI, or short codes explode the series count. Enforced in middleware
  and asserted in tests.
- **Global-recorder test fragility** — only one recorder installs per process;
  the `OnceLock` idempotency guard is the mitigation, exercised by parallel tests.
