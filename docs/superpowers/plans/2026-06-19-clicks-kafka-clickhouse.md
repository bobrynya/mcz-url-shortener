# Clicks on Kafka + ClickHouse — Implementation Plan (Phase 1)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move click recording off PostgreSQL onto a Kafka → Rust-consumer → ClickHouse pipeline, with a degraded mode that keeps redirects working when Kafka/ClickHouse are down.

**Architecture:** The redirect handler publishes click events to a Kafka topic via a `ClickPublisher` port (Kafka producer). A background Rust consumer reads the topic, batches events, and bulk-inserts into a ClickHouse `MergeTree` table. Statistics are read from ClickHouse via a `ClickStatsReader` port, while link/domain metadata stays in PostgreSQL; `StatsService` merges the two. The in-process mpsc worker, `PgStatsRepository`, and the `link_clicks` table are removed.

**Tech Stack:** Rust 1.96 / edition 2024, axum 0.8, `rdkafka` (Kafka client, vendored librdkafka), `clickhouse` (official async client over HTTP), tokio, sqlx (Postgres only now), `metrics`, `tracing`.

## Global Constraints

- MSRV **1.96**, Rust **edition 2024** (pinned via `rust-toolchain.toml`) — every new crate must build on this.
- No `unwrap()` / `expect()` outside tests, const init, or documented panic-safe contexts.
- Errors use `AppError` (`src/error.rs`); async traits use `#[async_trait]`.
- Logging via `tracing::{info,warn,error,debug}` — no `println!`. Metrics via `metrics::counter!` / `histogram!`.
- Clippy is `-D warnings`; `cargo fmt` before every commit.
- Imports grouped `std` → external → `crate::` with blank lines between groups.
- Repository **traits** live in `domain/`, **implementations** in `infrastructure/`. Handlers delegate to services, no business logic.
- After any `sqlx::query!` / schema change run `cargo sqlx prepare` to refresh `.sqlx/`.
- Commit messages: `<type>: <subject>` imperative. Never push without explicit user confirmation.
- ClickHouse delivery is **at-least-once**: commit Kafka offsets only after a successful insert; rare duplicates are acceptable (plain `MergeTree`).
- Critical dependency for health = **PostgreSQL** only. Kafka, ClickHouse, Redis are **non-critical** (service stays up, `/health` → 200 `degraded`).

---

## File Structure

**Created:**
- `src/domain/repositories/click_publisher.rs` — `ClickPublisher` trait (write side) + mock.
- `src/domain/repositories/click_stats_reader.rs` — `ClickStatsReader` trait (read side) + mock + `ClickAggRow`.
- `src/infrastructure/messaging/mod.rs` — messaging module root.
- `src/infrastructure/messaging/kafka_producer.rs` — reconnecting Kafka producer + `KafkaClickPublisher` + `NoopClickPublisher`.
- `src/infrastructure/messaging/click_consumer.rs` — Kafka→ClickHouse batch consumer (`BatchBuffer`, `decode_event`, `run_click_consumer`).
- `src/infrastructure/persistence/clickhouse_client.rs` — reconnecting ClickHouse client + `ClickRow` + `ClickSink`.
- `src/infrastructure/persistence/clickhouse_stats_reader.rs` — `ClickHouseStatsReader` (impl `ClickStatsReader`) + `UnavailableStatsReader`.
- `docker/clickhouse/init/01_schema.sql` — ClickHouse `clicks` MergeTree schema.
- `migrations/20260620000000_drop_link_clicks.sql` — drop the PG clicks table.

**Modified:**
- `Cargo.toml` — add `rdkafka`, `clickhouse`.
- `src/config.rs` — add Kafka/ClickHouse/batch config; remove `click_queue_capacity`, `click_worker_concurrency` (in switchover task).
- `src/error.rs` — add `AppError::ServiceUnavailable` (503).
- `src/domain/click_event.rs` — add `link_id`, `clicked_at`, derive `Serialize`/`Deserialize`.
- `src/domain/mod.rs` — drop `click_worker`; update docs.
- `src/domain/repositories/mod.rs` — export new ports; drop `StatsRepository`.
- `src/domain/repositories/stats_repository.rs` — keep `StatsFilter`/`LinkStats`/`DetailedStats`, remove the `StatsRepository` trait.
- `src/domain/entities/click.rs` — remove `NewClick` (no longer written from app).
- `src/application/services/stats_service.rs` — rework onto `ClickStatsReader` + `LinkRepository`.
- `src/api/handlers/redirect.rs` — publish via `ClickPublisher`; cache value carries `link_id`.
- `src/api/handlers/health.rs` — critical/non-critical split; add Kafka + ClickHouse checks; drop click-queue check.
- `src/api/dto/health.rs` — add Kafka + ClickHouse check fields.
- `src/state.rs` — replace `click_sender` with `click_publisher`; rework `StatsService` generics.
- `src/server.rs` — init Kafka/ClickHouse, spawn consumer, remove worker.
- `src/infrastructure/persistence/mod.rs` — export ClickHouse types, drop `PgStatsRepository`.
- `src/infrastructure/mod.rs` — add `messaging` module.
- `src/domain/repositories/link_repository.rs` — add `count_all_links` (move from stats) — actually reuse existing `count(None)`; see Task 7.
- `Dockerfile` — add build deps for librdkafka in builder stage.
- `docker-compose.yml` — add `kafka` + `clickhouse` services; pass new env to `app`.
- `.env.example` — document new variables.

**Deleted:**
- `src/domain/click_worker.rs`
- `src/infrastructure/persistence/pg_stats_repository.rs`

---

## Task 1: Add dependencies and verify they build on MSRV 1.96

This is a build-spike task: prove `rdkafka` and `clickhouse` compile on the pinned toolchain before writing code against them. Failure here changes crate choices.

**Files:**
- Modify: `Cargo.toml`

**Interfaces:**
- Produces: crates `rdkafka` and `clickhouse` available to later tasks.

- [ ] **Step 1: Add the dependencies**

In `Cargo.toml` under `[dependencies]`, after the `redis = ...` line, add:

```toml
# Messaging / analytics
rdkafka = { version = "0.37", default-features = false, features = ["cmake-build", "tokio", "libz"] }
clickhouse = { version = "0.13", default-features = false, features = ["lz4", "chrono"] }
```

Rationale: `cmake-build` vendors and statically links librdkafka (no system package at runtime); plaintext-only (no `ssl`/`gssapi`) keeps the C build minimal for local Kafka.

- [ ] **Step 2: Verify it compiles on the pinned toolchain**

Run: `cargo +1.96 check 2>&1 | tail -20` (or just `cargo check` if 1.96 is the default via `rust-toolchain.toml`).
Expected: PASS (compiles). You need `cmake` and a C/C++ compiler locally; on macOS install with `brew install cmake` if missing.

If `rdkafka` or `clickhouse` fail on 1.96/edition 2024, STOP and report — try the next minor version, or fall back (`rdkafka` 0.36). Do not proceed until both compile.

- [ ] **Step 3: Commit**

```bash
cargo fmt
git add Cargo.toml Cargo.lock
git commit -m "chore: add rdkafka and clickhouse dependencies"
```

---

## Task 2: Add `AppError::ServiceUnavailable` (503)

Statistics endpoints must return 503 when ClickHouse is unavailable.

**Files:**
- Modify: `src/error.rs`
- Test: `src/error.rs` (in-module `#[cfg(test)]`)

**Interfaces:**
- Produces: `AppError::service_unavailable(message: impl Into<String>, details: Value) -> AppError`, mapping to HTTP 503, error code `"service_unavailable"`.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/error.rs`:

```rust
#[test]
fn test_service_unavailable_is_503() {
    assert_eq!(
        status(AppError::service_unavailable("clickhouse down", json!({}))),
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[test]
fn test_service_unavailable_code() {
    assert_eq!(
        AppError::service_unavailable("x", json!({})).to_error_info().code,
        "service_unavailable"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib error::tests::test_service_unavailable_is_503`
Expected: FAIL — no function `service_unavailable`.

- [ ] **Step 3: Implement the variant**

In `src/error.rs`:

1. Add to the `AppError` enum (after `Internal { .. }`):

```rust
    ServiceUnavailable { message: String, details: Value },
```

2. Add a constructor in `impl AppError` (after `unauthorized`):

```rust
    /// Creates a service unavailable error (503) for non-critical dependency outages.
    pub fn service_unavailable(message: impl Into<String>, details: Value) -> Self {
        Self::ServiceUnavailable {
            message: message.into(),
            details,
        }
    }
```

3. Add the arm in `to_error_info`:

```rust
            AppError::ServiceUnavailable { message, details } => {
                ("service_unavailable", message, details)
            }
```

4. Add the arm in `IntoResponse::into_response` (returns false for `add_www_authenticate`):

```rust
            AppError::ServiceUnavailable { message, details } => (
                StatusCode::SERVICE_UNAVAILABLE,
                "service_unavailable",
                message,
                details,
                false,
            ),
```

5. Add the arm in `Display::fmt`:

```rust
            AppError::ServiceUnavailable { message, .. } => {
                write!(f, "Service unavailable: {}", message)
            }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib error::`
Expected: PASS (all error tests).

- [ ] **Step 5: Commit**

```bash
cargo fmt && cargo clippy -- -D warnings
git add src/error.rs
git commit -m "feat: add ServiceUnavailable (503) error variant"
```

---

## Task 3: Extend `ClickEvent` with `link_id` + `clicked_at` and make it serializable

The event becomes the Kafka payload. It carries `link_id` (resolved at redirect time) and the click timestamp; `domain`/`code` are removed.

**Files:**
- Modify: `src/domain/click_event.rs`
- Modify: `src/api/handlers/redirect.rs` (call site only, to keep compiling)
- Modify: `src/domain/click_worker.rs` (call sites in its tests, to keep compiling — worker is deleted later in Task 9)

**Interfaces:**
- Produces: `ClickEvent { link_id: i64, ip: Option<String>, user_agent: Option<String>, referer: Option<String>, clicked_at: DateTime<Utc> }`, deriving `Serialize, Deserialize, Debug, Clone`, with `ClickEvent::new(link_id, ip, user_agent, referer, clicked_at)`.

- [ ] **Step 1: Replace the test module with serde round-trip tests**

Replace the entire `#[cfg(test)] mod tests { ... }` in `src/domain/click_event.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn sample() -> ClickEvent {
        ClickEvent::new(
            42,
            Some("192.168.1.1".to_string()),
            Some("Mozilla/5.0".to_string()),
            Some("https://google.com".to_string()),
            Utc.with_ymd_and_hms(2026, 6, 19, 12, 0, 0).unwrap(),
        )
    }

    #[test]
    fn test_click_event_fields() {
        let e = sample();
        assert_eq!(e.link_id, 42);
        assert_eq!(e.ip.as_deref(), Some("192.168.1.1"));
        assert_eq!(e.user_agent.as_deref(), Some("Mozilla/5.0"));
        assert_eq!(e.referer.as_deref(), Some("https://google.com"));
    }

    #[test]
    fn test_click_event_json_round_trip() {
        let e = sample();
        let json = serde_json::to_string(&e).unwrap();
        let back: ClickEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.link_id, e.link_id);
        assert_eq!(back.ip, e.ip);
        assert_eq!(back.user_agent, e.user_agent);
        assert_eq!(back.referer, e.referer);
        assert_eq!(back.clicked_at, e.clicked_at);
    }

    #[test]
    fn test_click_event_minimal_serialization() {
        let e = ClickEvent::new(7, None, None, None, Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap());
        let json = serde_json::to_string(&e).unwrap();
        let back: ClickEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.link_id, 7);
        assert!(back.ip.is_none());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib domain::click_event 2>&1 | tail -20`
Expected: FAIL — `new` signature mismatch / missing `Serialize`.

- [ ] **Step 3: Rewrite the struct and constructor**

Replace the struct + `impl ClickEvent` (lines for `pub struct ClickEvent` through the end of `impl ClickEvent`) in `src/domain/click_event.rs` with:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A click event published to Kafka and consumed into ClickHouse.
///
/// `link_id` is resolved at redirect time (from the loaded link or the cache),
/// so the consumer never needs to touch PostgreSQL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClickEvent {
    pub link_id: i64,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    pub referer: Option<String>,
    pub clicked_at: DateTime<Utc>,
}

impl ClickEvent {
    /// Creates a new click event.
    pub fn new(
        link_id: i64,
        ip: Option<String>,
        user_agent: Option<String>,
        referer: Option<String>,
        clicked_at: DateTime<Utc>,
    ) -> Self {
        Self {
            link_id,
            ip,
            user_agent,
            referer,
            clicked_at,
        }
    }
}
```

(Remove the old module-level doc-comment lines referencing `domain`/`code` if they no longer apply; keep the file's `//!` header.)

- [ ] **Step 4: Fix the redirect handler call site (compile only)**

In `src/api/handlers/redirect.rs`, replace the `ClickEvent::new(...)` block (currently passing `domain, code, ip, user_agent, referer`) with a `link_id`-based call. For now the redirect still resolves `link_id` on the cache-miss path only; on cache HIT we don't yet have it. To keep this task compiling and behavior unchanged until Task 6, temporarily skip publishing on cache HIT by capturing an `Option<i64>` link id:

Change the cache-miss arm to also yield the id, and the HIT arm to yield `None`. Concretely, change the `match state.cache.get_url(...)` to bind `(long_url, permanent, link_id_opt)`:
- HIT arm returns `(url, permanent, None)`.
- MISS arm returns `(url, permanent, Some(link.id))` (the `link` is already loaded).
- Err arm returns `(link.long_url, link.permanent, Some(link.id))`.

Then replace the publish block with:

```rust
    if let Some(link_id) = link_id_opt {
        let click_event = ClickEvent::new(
            link_id,
            Some(addr.ip().to_string()),
            headers
                .get(header::USER_AGENT)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string()),
            headers
                .get(header::REFERER)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string()),
            chrono::Utc::now(),
        );

        if state.click_sender.try_send(click_event).is_err() {
            metrics::counter!("click_enqueue_dropped_total").increment(1);
            debug!("Click queue full; dropped click event for {}", cache_key);
        }
    }
```

> Note: the cache-value-carries-`link_id` change and switching to `ClickPublisher` happen in Tasks 6/9. This step only keeps the build green with the new event shape.

- [ ] **Step 5: Fix the click_worker test call sites (compile only)**

`src/domain/click_worker.rs` is deleted in Task 9, but must compile until then. Its `process_click` reads `event.domain`/`event.code`, which no longer exist. To avoid a large throwaway rewrite, gate the whole file out now: at the very top of `src/domain/click_worker.rs` add:

```rust
#![cfg(any())] // Deprecated: replaced by Kafka→ClickHouse consumer (see Task 9). Compiled out.
```

and in `src/domain/mod.rs` leave `pub mod click_worker;` (the `cfg(any())` makes it an empty module). This removes it from compilation without deleting yet, keeping history clean.

- [ ] **Step 6: Run the build + tests**

Run: `cargo test --lib domain::click_event && cargo check`
Expected: PASS for the click_event tests; `cargo check` compiles.

- [ ] **Step 7: Commit**

```bash
cargo fmt && cargo clippy -- -D warnings
git add src/domain/click_event.rs src/api/handlers/redirect.rs src/domain/click_worker.rs
git commit -m "refactor: ClickEvent carries link_id and is serializable"
```

---

## Task 4: Define the `ClickPublisher` and `ClickStatsReader` ports

Two narrow traits split write (Kafka) from read (ClickHouse), per the design.

**Files:**
- Create: `src/domain/repositories/click_publisher.rs`
- Create: `src/domain/repositories/click_stats_reader.rs`
- Modify: `src/domain/repositories/mod.rs`

**Interfaces:**
- Produces:
  - `trait ClickPublisher: Send + Sync { async fn publish(&self, event: ClickEvent) -> Result<(), AppError>; }` + `MockClickPublisher`.
  - `struct ClickAgg { pub link_id: i64, pub total: i64 }`
  - `trait ClickStatsReader: Send + Sync { async fn count_clicks(&self, link_id: i64, filter: &StatsFilter) -> Result<i64, AppError>; async fn list_clicks(&self, link_id: i64, filter: &StatsFilter) -> Result<Vec<Click>, AppError>; async fn counts_for_links(&self, link_ids: &[i64], filter: &StatsFilter) -> Result<std::collections::HashMap<i64, i64>, AppError>; }` + `MockClickStatsReader`.

- [ ] **Step 1: Write the publisher port**

Create `src/domain/repositories/click_publisher.rs`:

```rust
//! Write-side port for publishing click events (Kafka).

use async_trait::async_trait;

use crate::domain::click_event::ClickEvent;
use crate::error::AppError;

/// Publishes click events to the messaging backbone.
///
/// Implementations are fire-and-forget from the caller's perspective: when the
/// backend is unavailable the click is dropped (logged + metered) and `Ok(())`
/// is returned so the redirect never blocks or fails.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait ClickPublisher: Send + Sync {
    /// Publishes a single click event.
    async fn publish(&self, event: ClickEvent) -> Result<(), AppError>;
}
```

- [ ] **Step 2: Write the reader port**

Create `src/domain/repositories/click_stats_reader.rs`:

```rust
//! Read-side port for click statistics (ClickHouse).

use std::collections::HashMap;

use async_trait::async_trait;

use crate::domain::entities::Click;
use crate::domain::repositories::StatsFilter;
use crate::error::AppError;

/// Reads click analytics from the columnar store.
///
/// All methods return [`AppError::ServiceUnavailable`] when the store is down,
/// which surfaces to clients as HTTP 503.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait ClickStatsReader: Send + Sync {
    /// Counts clicks for one link within the filter's date range.
    async fn count_clicks(&self, link_id: i64, filter: &StatsFilter) -> Result<i64, AppError>;

    /// Lists paginated click records for one link, newest first.
    async fn list_clicks(&self, link_id: i64, filter: &StatsFilter) -> Result<Vec<Click>, AppError>;

    /// Returns per-link click totals for a set of links (used by `get_all_stats`).
    /// Links with no clicks are simply absent from the map.
    async fn counts_for_links(
        &self,
        link_ids: &[i64],
        filter: &StatsFilter,
    ) -> Result<HashMap<i64, i64>, AppError>;
}
```

- [ ] **Step 3: Wire the modules**

In `src/domain/repositories/mod.rs`:
- Add `pub mod click_publisher;` and `pub mod click_stats_reader;` next to the other `pub mod` lines.
- Add exports:

```rust
pub use click_publisher::ClickPublisher;
pub use click_stats_reader::ClickStatsReader;
```

- Add test-only mock exports:

```rust
#[cfg(test)]
pub use click_publisher::MockClickPublisher;
#[cfg(test)]
pub use click_stats_reader::MockClickStatsReader;
```

(Leave the existing `StatsRepository` export for now; removed in Task 7.)

- [ ] **Step 4: Verify it compiles**

Run: `cargo check`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt && cargo clippy -- -D warnings
git add src/domain/repositories/
git commit -m "feat: add ClickPublisher and ClickStatsReader ports"
```

---

## Task 5: ClickHouse client, row type, and `ClickSink`

The reconnecting client is shared by the reader (queries) and the consumer (inserts). `ClickRow` is the ClickHouse-mapped row; `ClickSink` is the insert abstraction that makes the consumer's batching unit-testable.

**Files:**
- Create: `src/infrastructure/persistence/clickhouse_client.rs`
- Modify: `src/infrastructure/persistence/mod.rs`

**Interfaces:**
- Produces:
  - `struct ClickRow { link_id: u64, ip: Option<String>, user_agent: Option<String>, referer: Option<String>, clicked_at: DateTime<Utc> }` deriving `clickhouse::Row, Serialize, Deserialize`, with `clicked_at` using `#[serde(with = "clickhouse::serde::chrono::datetime64::millis")]`. `From<&ClickEvent> for ClickRow`.
  - `trait ClickSink: Send + Sync { async fn insert_batch(&self, rows: &[ClickRow]) -> Result<(), AppError>; }`
  - `struct ReconnectingClickHouse` with `new(cfg) -> Self`, `async fn get(&self) -> Option<clickhouse::Client>`, `async fn health_check(&self) -> bool`; implements `ClickSink`.

- [ ] **Step 1: Write the row + From test**

Create `src/infrastructure/persistence/clickhouse_client.rs` starting with imports and the row type, plus a test:

```rust
//! Reconnecting ClickHouse client, row mapping, and insert sink.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use clickhouse::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::domain::click_event::ClickEvent;
use crate::error::AppError;

/// A click row as stored in ClickHouse (`url_shortener.clicks`).
#[derive(Debug, Clone, clickhouse::Row, Serialize, Deserialize)]
pub struct ClickRow {
    pub link_id: u64,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    pub referer: Option<String>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub clicked_at: DateTime<Utc>,
}

impl From<&ClickEvent> for ClickRow {
    fn from(e: &ClickEvent) -> Self {
        Self {
            // link_id is a positive bigint from Postgres; cast to UInt64.
            link_id: e.link_id.max(0) as u64,
            ip: e.ip.clone(),
            user_agent: e.user_agent.clone(),
            referer: e.referer.clone(),
            clicked_at: e.clicked_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_click_row_from_event() {
        let e = ClickEvent::new(
            99,
            Some("1.2.3.4".to_string()),
            Some("UA".to_string()),
            None,
            Utc.with_ymd_and_hms(2026, 6, 19, 0, 0, 0).unwrap(),
        );
        let row = ClickRow::from(&e);
        assert_eq!(row.link_id, 99);
        assert_eq!(row.ip.as_deref(), Some("1.2.3.4"));
        assert!(row.referer.is_none());
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib clickhouse_client::tests::test_click_row_from_event 2>&1 | tail -20`
Expected: FAIL — module not declared yet.

- [ ] **Step 3: Declare the module**

In `src/infrastructure/persistence/mod.rs` add `pub mod clickhouse_client;` and:

```rust
pub use clickhouse_client::{ClickRow, ClickSink, ReconnectingClickHouse};
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --lib clickhouse_client::tests::test_click_row_from_event`
Expected: PASS.

- [ ] **Step 5: Add the reconnecting client + sink**

Append to `src/infrastructure/persistence/clickhouse_client.rs` (after the `From` impl, before `#[cfg(test)]`):

```rust
/// Connection settings for ClickHouse (HTTP interface).
#[derive(Debug, Clone)]
pub struct ClickHouseConfig {
    pub url: String,
    pub database: String,
    pub user: String,
    pub password: String,
    pub retry_interval: Duration,
}

/// Insert abstraction so the consumer's batching is testable without a real DB.
#[async_trait]
pub trait ClickSink: Send + Sync {
    /// Inserts a batch of click rows. Returns `Err` only on a real failure
    /// (caller must NOT commit Kafka offsets on error).
    async fn insert_batch(&self, rows: &[ClickRow]) -> Result<(), AppError>;
}

/// ClickHouse client with lazy connection and a retry cooldown.
///
/// If ClickHouse is down at startup the service keeps serving redirects; reads
/// return 503 and inserts fail (offsets stay uncommitted) until it recovers.
pub struct ReconnectingClickHouse {
    cfg: ClickHouseConfig,
    client: Mutex<Option<Client>>,
    next_attempt_at: Mutex<Instant>,
}

impl ReconnectingClickHouse {
    pub fn new(cfg: ClickHouseConfig) -> Self {
        Self {
            cfg,
            client: Mutex::new(None),
            next_attempt_at: Mutex::new(Instant::now()),
        }
    }

    fn build_client(&self) -> Client {
        Client::default()
            .with_url(&self.cfg.url)
            .with_database(&self.cfg.database)
            .with_user(&self.cfg.user)
            .with_password(&self.cfg.password)
    }

    /// Returns a connected client, or `None` while ClickHouse is unavailable.
    pub async fn get(&self) -> Option<Client> {
        {
            let guard = self.client.lock().await;
            if let Some(c) = guard.as_ref() {
                return Some(c.clone());
            }
        }

        let mut next = self.next_attempt_at.lock().await;
        if Instant::now() < *next {
            return None;
        }
        *next = Instant::now() + self.cfg.retry_interval;

        let candidate = self.build_client();
        match candidate.query("SELECT 1").execute().await {
            Ok(()) => {
                let mut guard = self.client.lock().await;
                *guard = Some(candidate.clone());
                tracing::info!("ClickHouse connected");
                Some(candidate)
            }
            Err(e) => {
                tracing::warn!(error = %e, "ClickHouse unavailable");
                None
            }
        }
    }

    /// Drops the cached client so the next `get()` reconnects.
    async fn invalidate(&self) {
        *self.client.lock().await = None;
    }

    /// Health probe used by `/health` (non-critical).
    pub async fn health_check(&self) -> bool {
        self.get().await.is_some()
    }
}

#[async_trait]
impl ClickSink for ReconnectingClickHouse {
    async fn insert_batch(&self, rows: &[ClickRow]) -> Result<(), AppError> {
        if rows.is_empty() {
            return Ok(());
        }
        let client = self.get().await.ok_or_else(|| {
            AppError::service_unavailable("ClickHouse is unavailable", serde_json::json!({}))
        })?;

        let mut insert = client.insert("clicks").map_err(|e| {
            AppError::service_unavailable("ClickHouse insert failed", serde_json::json!({ "error": e.to_string() }))
        })?;
        for row in rows {
            insert.write(row).await.map_err(|e| {
                AppError::service_unavailable("ClickHouse write failed", serde_json::json!({ "error": e.to_string() }))
            })?;
        }
        match insert.end().await {
            Ok(()) => Ok(()),
            Err(e) => {
                self.invalidate().await;
                Err(AppError::service_unavailable(
                    "ClickHouse commit failed",
                    serde_json::json!({ "error": e.to_string() }),
                ))
            }
        }
    }
}
```

Also export the config: update the `pub use` in `mod.rs` to:

```rust
pub use clickhouse_client::{ClickHouseConfig, ClickRow, ClickSink, ReconnectingClickHouse};
```

- [ ] **Step 6: Verify compilation**

Run: `cargo check && cargo test --lib clickhouse_client::`
Expected: PASS. (Connection-dependent paths are exercised in Task 8 integration, not here.)

> If the `clickhouse` 0.13 API differs (`insert.write` signature, `datetime64::millis` path), adjust to the crate's actual API — verify with `cargo doc -p clickhouse --open` and keep the same behavior.

- [ ] **Step 7: Commit**

```bash
cargo fmt && cargo clippy -- -D warnings
git add src/infrastructure/persistence/
git commit -m "feat: reconnecting ClickHouse client with insert sink"
```

---

## Task 6: ClickHouse stats reader (`ClickStatsReader` impl)

Implements the read-side port with ClickHouse queries, plus an `UnavailableStatsReader` for when ClickHouse is not configured.

**Files:**
- Create: `src/infrastructure/persistence/clickhouse_stats_reader.rs`
- Modify: `src/infrastructure/persistence/mod.rs`

**Interfaces:**
- Consumes: `ReconnectingClickHouse`, `StatsFilter`, `Click`, `ClickStatsReader`.
- Produces: `ClickHouseStatsReader::new(Arc<ReconnectingClickHouse>)` impl `ClickStatsReader`; `UnavailableStatsReader` impl `ClickStatsReader` (always 503).

- [ ] **Step 1: Write the unavailable-reader test**

Create `src/infrastructure/persistence/clickhouse_stats_reader.rs`:

```rust
//! ClickHouse implementation of the click statistics read port.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::domain::entities::Click;
use crate::domain::repositories::{ClickStatsReader, StatsFilter};
use crate::error::AppError;
use crate::infrastructure::persistence::ReconnectingClickHouse;

/// Reader used when ClickHouse is not configured — every call is a 503.
pub struct UnavailableStatsReader;

#[async_trait]
impl ClickStatsReader for UnavailableStatsReader {
    async fn count_clicks(&self, _link_id: i64, _filter: &StatsFilter) -> Result<i64, AppError> {
        Err(unavailable())
    }
    async fn list_clicks(&self, _link_id: i64, _filter: &StatsFilter) -> Result<Vec<Click>, AppError> {
        Err(unavailable())
    }
    async fn counts_for_links(
        &self,
        _link_ids: &[i64],
        _filter: &StatsFilter,
    ) -> Result<HashMap<i64, i64>, AppError> {
        Err(unavailable())
    }
}

fn unavailable() -> AppError {
    AppError::service_unavailable("ClickHouse is not configured", serde_json::json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_unavailable_reader_returns_503() {
        let r = UnavailableStatsReader;
        let f = StatsFilter::new(0, 10);
        let err = r.count_clicks(1, &f).await.unwrap_err();
        assert!(matches!(err, AppError::ServiceUnavailable { .. }));
    }
}
```

- [ ] **Step 2: Declare module + run test (fail → pass)**

In `mod.rs` add `pub mod clickhouse_stats_reader;` and `pub use clickhouse_stats_reader::{ClickHouseStatsReader, UnavailableStatsReader};` (the `ClickHouseStatsReader` name won't exist yet — add it in Step 3 before this compiles; so do Step 3 first, then run).

Run after Step 3: `cargo test --lib clickhouse_stats_reader::`
Expected: PASS.

- [ ] **Step 3: Implement the real reader**

Append to `src/infrastructure/persistence/clickhouse_stats_reader.rs` (before `#[cfg(test)]`):

```rust
/// Bounds used to translate `Option` date filters into concrete ClickHouse params.
const MIN_TS: &str = "1970-01-01 00:00:00.000";
const MAX_TS: &str = "9999-01-01 00:00:00.000";

fn from_bound(f: &StatsFilter) -> String {
    f.from_date.map(fmt_ts).unwrap_or_else(|| MIN_TS.to_string())
}
fn to_bound(f: &StatsFilter) -> String {
    f.to_date.map(fmt_ts).unwrap_or_else(|| MAX_TS.to_string())
}
fn fmt_ts(ts: DateTime<Utc>) -> String {
    ts.format("%Y-%m-%d %H:%M:%S%.3f").to_string()
}

/// Row returned when reading individual clicks.
#[derive(Debug, Deserialize, clickhouse::Row)]
struct ClickReadRow {
    link_id: u64,
    ip: Option<String>,
    user_agent: Option<String>,
    referer: Option<String>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    clicked_at: DateTime<Utc>,
}

/// Row returned when aggregating per-link counts.
#[derive(Debug, Deserialize, clickhouse::Row)]
struct CountRow {
    link_id: u64,
    total: u64,
}

/// ClickHouse-backed implementation of [`ClickStatsReader`].
pub struct ClickHouseStatsReader {
    ch: Arc<ReconnectingClickHouse>,
}

impl ClickHouseStatsReader {
    pub fn new(ch: Arc<ReconnectingClickHouse>) -> Self {
        Self { ch }
    }

    fn client(&self) -> impl std::future::Future<Output = Result<clickhouse::Client, AppError>> + '_ {
        async move {
            self.ch.get().await.ok_or_else(|| {
                AppError::service_unavailable("ClickHouse is unavailable", serde_json::json!({}))
            })
        }
    }
}

#[async_trait]
impl ClickStatsReader for ClickHouseStatsReader {
    async fn count_clicks(&self, link_id: i64, filter: &StatsFilter) -> Result<i64, AppError> {
        let client = self.client().await?;
        let count: u64 = client
            .query(
                "SELECT count() FROM clicks \
                 WHERE link_id = ? AND clicked_at >= ? AND clicked_at <= ?",
            )
            .bind(link_id.max(0) as u64)
            .bind(from_bound(filter))
            .bind(to_bound(filter))
            .fetch_one()
            .await
            .map_err(map_ch_err)?;
        Ok(count as i64)
    }

    async fn list_clicks(&self, link_id: i64, filter: &StatsFilter) -> Result<Vec<Click>, AppError> {
        let client = self.client().await?;
        let rows: Vec<ClickReadRow> = client
            .query(
                "SELECT link_id, ip, user_agent, referer, clicked_at FROM clicks \
                 WHERE link_id = ? AND clicked_at >= ? AND clicked_at <= ? \
                 ORDER BY clicked_at DESC LIMIT ? OFFSET ?",
            )
            .bind(link_id.max(0) as u64)
            .bind(from_bound(filter))
            .bind(to_bound(filter))
            .bind(filter.limit.max(0) as u64)
            .bind(filter.offset.max(0) as u64)
            .fetch_all()
            .await
            .map_err(map_ch_err)?;

        Ok(rows
            .into_iter()
            .map(|r| {
                Click::new(
                    0,
                    r.link_id as i64,
                    r.clicked_at,
                    r.user_agent,
                    r.referer,
                    r.ip,
                )
            })
            .collect())
    }

    async fn counts_for_links(
        &self,
        link_ids: &[i64],
        filter: &StatsFilter,
    ) -> Result<HashMap<i64, i64>, AppError> {
        if link_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let client = self.client().await?;
        let ids: Vec<u64> = link_ids.iter().map(|id| (*id).max(0) as u64).collect();
        let rows: Vec<CountRow> = client
            .query(
                "SELECT link_id, count() AS total FROM clicks \
                 WHERE link_id IN ? AND clicked_at >= ? AND clicked_at <= ? \
                 GROUP BY link_id",
            )
            .bind(ids)
            .bind(from_bound(filter))
            .bind(to_bound(filter))
            .fetch_all()
            .await
            .map_err(map_ch_err)?;

        Ok(rows
            .into_iter()
            .map(|r| (r.link_id as i64, r.total as i64))
            .collect())
    }
}

fn map_ch_err(e: clickhouse::error::Error) -> AppError {
    tracing::warn!(error = %e, "ClickHouse query failed");
    AppError::service_unavailable("ClickHouse query failed", serde_json::json!({}))
}
```

- [ ] **Step 4: Run tests + check**

Run: `cargo test --lib clickhouse_stats_reader:: && cargo check`
Expected: PASS.

> Verify `.bind(Vec<u64>)` for `IN ?` and `.fetch_one::<u64>()` against the clickhouse 0.13 API; adjust binding style if needed (some versions want `fetch_one::<u64>()` turbofish).

- [ ] **Step 5: Commit**

```bash
cargo fmt && cargo clippy -- -D warnings
git add src/infrastructure/persistence/
git commit -m "feat: ClickHouse stats reader implementing ClickStatsReader"
```

---

## Task 7: Kafka producer and `ClickPublisher` implementations

Reconnecting Kafka producer that publishes click events; plus a `NoopClickPublisher` for when Kafka is not configured.

**Files:**
- Create: `src/infrastructure/messaging/mod.rs`
- Create: `src/infrastructure/messaging/kafka_producer.rs`
- Modify: `src/infrastructure/mod.rs`

**Interfaces:**
- Consumes: `ClickEvent`, `ClickPublisher`.
- Produces: `KafkaClickPublisher::new(brokers: &str, topic: String) -> Result<Self, AppError>` impl `ClickPublisher`; `NoopClickPublisher` impl `ClickPublisher`; `KafkaClickPublisher::health_check(&self) -> bool`.

- [ ] **Step 1: Write the Noop publisher test**

Create `src/infrastructure/messaging/kafka_producer.rs`:

```rust
//! Kafka click publisher (write side) with a no-op fallback.

use async_trait::async_trait;
use rdkafka::producer::{FutureProducer, FutureRecord};
use rdkafka::util::Timeout;
use rdkafka::ClientConfig;
use std::time::Duration;

use crate::domain::click_event::ClickEvent;
use crate::domain::repositories::ClickPublisher;
use crate::error::AppError;

/// Used when Kafka is not configured: clicks are counted and dropped.
pub struct NoopClickPublisher;

#[async_trait]
impl ClickPublisher for NoopClickPublisher {
    async fn publish(&self, _event: ClickEvent) -> Result<(), AppError> {
        metrics::counter!("click_publish_dropped_total", "reason" => "not_configured").increment(1);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[tokio::test]
    async fn test_noop_publisher_ok() {
        let p = NoopClickPublisher;
        let e = ClickEvent::new(1, None, None, None, Utc::now());
        assert!(p.publish(e).await.is_ok());
    }
}
```

- [ ] **Step 2: Declare module + run test (fail → pass)**

In `src/infrastructure/mod.rs` add `pub mod messaging;`. Create `src/infrastructure/messaging/mod.rs`:

```rust
//! Messaging infrastructure: Kafka producer and click consumer.

pub mod click_consumer;
pub mod kafka_producer;

pub use kafka_producer::{KafkaClickPublisher, NoopClickPublisher};
```

> `click_consumer` is created in Task 8; create an empty placeholder now so the module resolves:
> Create `src/infrastructure/messaging/click_consumer.rs` with a single line `//! Click consumer (implemented in the consumer task).` and nothing else for now (filled in Task 8).

Run: `cargo test --lib kafka_producer::tests::test_noop_publisher_ok`
Expected: PASS.

- [ ] **Step 3: Implement the Kafka publisher**

Append to `src/infrastructure/messaging/kafka_producer.rs` (before `#[cfg(test)]`):

```rust
/// Kafka-backed click publisher.
///
/// The `FutureProducer` connects lazily and reconnects internally, so a broker
/// outage doesn't need app-level reconnection logic — failed sends are dropped
/// (logged + metered) and `publish` still returns `Ok(())`.
pub struct KafkaClickPublisher {
    producer: FutureProducer,
    topic: String,
}

impl KafkaClickPublisher {
    /// Builds a producer for the given brokers.
    pub fn new(brokers: &str, topic: String) -> Result<Self, AppError> {
        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", brokers)
            .set("message.timeout.ms", "5000")
            .create()
            .map_err(|e| {
                AppError::internal(
                    "Failed to create Kafka producer",
                    serde_json::json!({ "error": e.to_string() }),
                )
            })?;
        Ok(Self { producer, topic })
    }

    /// Liveness probe for `/health` (non-critical): can we fetch metadata?
    pub fn health_check(&self) -> bool {
        self.producer
            .client()
            .fetch_metadata(Some(&self.topic), Timeout::After(Duration::from_secs(2)))
            .is_ok()
    }
}

#[async_trait]
impl ClickPublisher for KafkaClickPublisher {
    async fn publish(&self, event: ClickEvent) -> Result<(), AppError> {
        let payload = match serde_json::to_vec(&event) {
            Ok(p) => p,
            Err(e) => {
                tracing::error!(error = %e, "Failed to serialize click event");
                metrics::counter!("click_publish_dropped_total", "reason" => "serialize").increment(1);
                return Ok(());
            }
        };
        let key = event.link_id.to_string();
        let record = FutureRecord::to(&self.topic).payload(&payload).key(&key);

        match self.producer.send(record, Timeout::After(Duration::from_secs(1))).await {
            Ok(_) => {
                metrics::counter!("click_publish_total").increment(1);
            }
            Err((e, _msg)) => {
                tracing::warn!(error = %e, "Kafka publish failed; dropping click");
                metrics::counter!("click_publish_dropped_total", "reason" => "send").increment(1);
            }
        }
        Ok(())
    }
}
```

- [ ] **Step 4: Verify compilation + tests**

Run: `cargo check && cargo test --lib kafka_producer::`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt && cargo clippy -- -D warnings
git add src/infrastructure/
git commit -m "feat: Kafka click publisher with no-op fallback"
```

---

## Task 8: Click consumer (Kafka → ClickHouse batching)

The consumer decodes events, batches them, and bulk-inserts via `ClickSink`. Batching logic is unit-tested through `BatchBuffer` + `decode_event`; the rdkafka loop is thin glue.

**Files:**
- Modify: `src/infrastructure/messaging/click_consumer.rs`

**Interfaces:**
- Consumes: `ClickEvent`, `ClickRow`, `ClickSink`, `tokio_util::sync::CancellationToken`.
- Produces:
  - `fn decode_event(payload: &[u8]) -> Result<ClickEvent, serde_json::Error>`
  - `struct BatchBuffer { ... }` with `new(capacity: usize)`, `push(ClickRow) -> bool` (true if full), `take() -> Vec<ClickRow>`, `len() -> usize`, `is_empty() -> bool`.
  - `async fn run_click_consumer(brokers: String, group: String, topic: String, sink: Arc<dyn ClickSink>, batch_size: usize, flush: Duration, shutdown: CancellationToken)`.

- [ ] **Step 1: Add `tokio-util` dependency**

In `Cargo.toml` under `[dependencies]`, add (near `tokio`):

```toml
tokio-util = { version = "0.7", default-features = false, features = ["rt"] }
```

Run: `cargo check` → PASS. Commit later with this task.

- [ ] **Step 2: Write batching + decode unit tests**

Replace the placeholder `src/infrastructure/messaging/click_consumer.rs` with imports, the `BatchBuffer`/`decode_event` definitions, and tests:

```rust
//! Kafka → ClickHouse click consumer with size/time batching.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{CommitMode, Consumer, StreamConsumer};
use rdkafka::Message;
use tokio_util::sync::CancellationToken;

use crate::domain::click_event::ClickEvent;
use crate::infrastructure::persistence::{ClickRow, ClickSink};

/// Decodes a Kafka payload into a click event.
pub fn decode_event(payload: &[u8]) -> Result<ClickEvent, serde_json::Error> {
    serde_json::from_slice(payload)
}

/// Accumulates click rows until the configured capacity is reached.
pub struct BatchBuffer {
    capacity: usize,
    rows: Vec<ClickRow>,
}

impl BatchBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            rows: Vec::with_capacity(capacity.max(1)),
        }
    }

    /// Pushes a row; returns `true` when the buffer is full and should flush.
    pub fn push(&mut self, row: ClickRow) -> bool {
        self.rows.push(row);
        self.rows.len() >= self.capacity
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Drains the buffer, returning the accumulated rows.
    pub fn take(&mut self) -> Vec<ClickRow> {
        std::mem::take(&mut self.rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: u64) -> ClickRow {
        ClickRow {
            link_id: id,
            ip: None,
            user_agent: None,
            referer: None,
            clicked_at: Utc::now(),
        }
    }

    #[test]
    fn test_buffer_signals_full_at_capacity() {
        let mut b = BatchBuffer::new(2);
        assert!(!b.push(row(1)));
        assert!(b.push(row(2)));
        assert_eq!(b.len(), 2);
    }

    #[test]
    fn test_buffer_take_drains() {
        let mut b = BatchBuffer::new(4);
        b.push(row(1));
        b.push(row(2));
        let drained = b.take();
        assert_eq!(drained.len(), 2);
        assert!(b.is_empty());
    }

    #[test]
    fn test_decode_event_round_trip() {
        let e = ClickEvent::new(5, Some("1.1.1.1".into()), None, None, Utc::now());
        let bytes = serde_json::to_vec(&e).unwrap();
        let back = decode_event(&bytes).unwrap();
        assert_eq!(back.link_id, 5);
    }

    #[test]
    fn test_decode_event_rejects_garbage() {
        assert!(decode_event(b"not json").is_err());
    }
}
```

- [ ] **Step 3: Run the unit tests (fail → pass)**

Run: `cargo test --lib click_consumer::tests`
Expected: PASS (definitions are in the same step).

- [ ] **Step 4: Add the consumer loop**

Append to `src/infrastructure/messaging/click_consumer.rs` (before `#[cfg(test)]`):

```rust
/// Runs the Kafka→ClickHouse consumer until `shutdown` is cancelled.
///
/// Batches up to `batch_size` events or every `flush` interval, whichever comes
/// first, then bulk-inserts via `sink`. Offsets are committed only after a
/// successful insert (at-least-once). Malformed messages are skipped + metered.
pub async fn run_click_consumer(
    brokers: String,
    group: String,
    topic: String,
    sink: Arc<dyn ClickSink>,
    batch_size: usize,
    flush: Duration,
    shutdown: CancellationToken,
) {
    let consumer: StreamConsumer = match ClientConfig::new()
        .set("bootstrap.servers", &brokers)
        .set("group.id", &group)
        .set("enable.auto.commit", "false")
        .set("auto.offset.reset", "earliest")
        .create()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "Failed to create Kafka consumer; clicks will not be ingested");
            return;
        }
    };

    if let Err(e) = consumer.subscribe(&[&topic]) {
        tracing::error!(error = %e, topic = %topic, "Failed to subscribe to clicks topic");
        return;
    }
    tracing::info!(topic = %topic, batch_size, "Click consumer started");

    let mut buffer = BatchBuffer::new(batch_size);
    let mut ticker = tokio::time::interval(flush);

    loop {
        tokio::select! {
            _ = shutdown.cancelled() => {
                flush_batch(&consumer, &sink, &mut buffer).await;
                tracing::info!("Click consumer stopped");
                return;
            }
            _ = ticker.tick() => {
                flush_batch(&consumer, &sink, &mut buffer).await;
            }
            msg = consumer.recv() => {
                match msg {
                    Ok(m) => {
                        metrics::counter!("click_consumer_received_total").increment(1);
                        if let Some(payload) = m.payload() {
                            match decode_event(payload) {
                                Ok(event) => {
                                    if buffer.push(ClickRow::from(&event)) {
                                        flush_batch(&consumer, &sink, &mut buffer).await;
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!(error = %e, "Skipping malformed click message");
                                    metrics::counter!("click_consumer_invalid_total").increment(1);
                                    // Skipped messages are still acked via the batch commit below
                                    // (we commit consumer position on the next successful flush).
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Kafka consumer recv error");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        }
    }
}

/// Inserts the buffered rows and, on success, commits consumer offsets.
async fn flush_batch(consumer: &StreamConsumer, sink: &Arc<dyn ClickSink>, buffer: &mut BatchBuffer) {
    if buffer.is_empty() {
        return;
    }
    let rows = buffer.take();
    let n = rows.len();
    match sink.insert_batch(&rows).await {
        Ok(()) => {
            metrics::counter!("click_consumer_inserted_total").increment(n as u64);
            metrics::histogram!("click_consumer_batch_size").record(n as f64);
            if let Err(e) = consumer.commit_consumer_state(CommitMode::Async) {
                tracing::warn!(error = %e, "Failed to commit Kafka offsets after insert");
            }
        }
        Err(e) => {
            // Do NOT commit: messages will be re-delivered after ClickHouse recovers.
            tracing::warn!(error = %e, count = n, "ClickHouse insert failed; offsets left uncommitted");
            metrics::counter!("click_consumer_insert_failed_total").increment(1);
            // Re-buffer so we retry the same rows on the next flush.
            for row in rows {
                buffer.push(row);
            }
        }
    }
}
```

- [ ] **Step 5: Run tests + check**

Run: `cargo test --lib click_consumer:: && cargo check`
Expected: PASS.

> Verify `commit_consumer_state` exists in rdkafka 0.37 (alternative: `consumer.commit(&TopicPartitionList, CommitMode::Async)`). Adjust to the available API if needed, preserving "commit only after successful insert".

- [ ] **Step 6: Commit**

```bash
cargo fmt && cargo clippy -- -D warnings
git add Cargo.toml Cargo.lock src/infrastructure/messaging/click_consumer.rs
git commit -m "feat: Kafka to ClickHouse click consumer with batching"
```

---

## Task 9: Switchover — rewire StatsService, AppState, server, health; remove PG clicks

The atomic change: `StatsService` reads from ClickHouse + Postgres metadata; `AppState` swaps the click channel for a publisher; `server.rs` spawns the consumer; health gets critical/non-critical split; old worker/repo/table are removed. This is one reviewable unit; keep the build green at the end.

**Files:**
- Modify: `src/application/services/stats_service.rs`
- Modify: `src/domain/repositories/stats_repository.rs` (remove trait, keep types)
- Modify: `src/domain/repositories/mod.rs`
- Modify: `src/domain/entities/click.rs` (remove `NewClick`)
- Modify: `src/state.rs`
- Modify: `src/server.rs`
- Modify: `src/config.rs`
- Modify: `src/api/handlers/redirect.rs`
- Modify: `src/api/handlers/health.rs`, `src/api/dto/health.rs`
- Modify: `src/infrastructure/persistence/mod.rs`, `src/domain/mod.rs`
- Create: `migrations/20260620000000_drop_link_clicks.sql`
- Delete: `src/domain/click_worker.rs`, `src/infrastructure/persistence/pg_stats_repository.rs`

**Interfaces:**
- Consumes: `ClickStatsReader`, `ClickPublisher`, `LinkRepository`, `DomainRepository`.
- Produces:
  - `StatsService<R: ClickStatsReader, L: LinkRepository>` with `new(reader: Arc<R>, links: Arc<L>) -> Self` and methods `get_detailed_stats(&self, code, filter) -> Result<DetailedStats, AppError>`, `get_all_stats(&self, filter) -> Result<Vec<LinkStats>, AppError>`, `count_all_links(&self) -> Result<i64, AppError>`.
  - `AppState.click_publisher: Arc<dyn ClickPublisher>` (replaces `click_sender`).

- [ ] **Step 1: Rework `StatsService` with TDD (merge logic)**

Replace the whole body of `src/application/services/stats_service.rs` with the new service + mock-based tests. The key behaviors: `get_detailed_stats` resolves the link via `LinkRepository` (404 if absent) then pulls count + clicks from the reader; `get_all_stats` pages links from `LinkRepository`, fetches per-link counts from the reader, and merges (missing → 0).

```rust
//! Click statistics service: merges Postgres link metadata with ClickHouse analytics.

use std::sync::Arc;

use serde_json::json;

use crate::domain::repositories::{
    ClickStatsReader, DetailedStats, LinkRepository, LinkStats, StatsFilter,
};
use crate::error::AppError;

/// Page size used internally when paginating links for `get_all_stats`.
/// `StatsFilter.offset`/`limit` already encode the requested page.
pub struct StatsService<R: ClickStatsReader, L: LinkRepository> {
    reader: Arc<R>,
    links: Arc<L>,
}

impl<R: ClickStatsReader, L: LinkRepository> StatsService<R, L> {
    pub fn new(reader: Arc<R>, links: Arc<L>) -> Self {
        Self { reader, links }
    }

    /// Detailed stats for one short code: metadata from PG, clicks from ClickHouse.
    pub async fn get_detailed_stats(
        &self,
        code: &str,
        filter: StatsFilter,
    ) -> Result<DetailedStats, AppError> {
        let link = self
            .links
            .find_by_code(code, filter.domain_id.unwrap_or(0))
            .await?;

        // When no domain filter is supplied we still need to resolve by code alone;
        // find_by_code requires a domain_id, so fall back to a code-only lookup.
        let link = match link {
            Some(l) => l,
            None if filter.domain_id.is_none() => self
                .links
                .find_any_by_code(code)
                .await?
                .ok_or_else(|| AppError::not_found("Statistics not found", json!({ "code": code })))?,
            None => {
                return Err(AppError::not_found(
                    "Statistics not found",
                    json!({ "code": code }),
                ))
            }
        };

        let total = self.reader.count_clicks(link.id, &filter).await?;
        let items = self.reader.list_clicks(link.id, &filter).await?;

        Ok(DetailedStats { link, total, items })
    }

    /// Aggregated per-link stats: page links from PG, counts from ClickHouse, merge.
    pub async fn get_all_stats(&self, filter: StatsFilter) -> Result<Vec<LinkStats>, AppError> {
        let page = (filter.offset / filter.limit.max(1)) + 1;
        let links = self
            .links
            .list(page, filter.limit, filter.domain_id)
            .await?;

        let ids: Vec<i64> = links.iter().map(|l| l.id).collect();
        let counts = self.reader.counts_for_links(&ids, &filter).await?;

        Ok(links
            .into_iter()
            .map(|l| LinkStats {
                link_id: l.id,
                code: l.code,
                domain: l.domain,
                long_url: l.long_url,
                total: counts.get(&l.id).copied().unwrap_or(0),
                created_at: l.created_at,
            })
            .collect())
    }

    /// Total link count (Postgres), for pagination metadata.
    pub async fn count_all_links(&self) -> Result<i64, AppError> {
        self.links.count(None).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::Link;
    use crate::domain::repositories::{MockClickStatsReader, MockLinkRepository};
    use chrono::Utc;
    use std::collections::HashMap;

    fn link(id: i64, code: &str) -> Link {
        Link::new(
            id,
            code.to_string(),
            "https://example.com".to_string(),
            Some("s.example.com".to_string()),
            Utc::now(),
            None,
            false,
            None,
        )
    }

    #[tokio::test]
    async fn test_get_all_stats_merges_counts_and_defaults_zero() {
        let mut links = MockLinkRepository::new();
        links
            .expect_list()
            .returning(|_, _, _| Ok(vec![link(1, "aaa"), link(2, "bbb")]));

        let mut reader = MockClickStatsReader::new();
        reader.expect_counts_for_links().returning(|_, _| {
            let mut m = HashMap::new();
            m.insert(1, 10);
            Ok(m) // link 2 absent → should default to 0
        });

        let svc = StatsService::new(Arc::new(reader), Arc::new(links));
        let out = svc.get_all_stats(StatsFilter::new(0, 25)).await.unwrap();

        assert_eq!(out.len(), 2);
        assert_eq!(out[0].total, 10);
        assert_eq!(out[1].total, 0);
    }

    #[tokio::test]
    async fn test_get_detailed_stats_not_found() {
        let mut links = MockLinkRepository::new();
        links.expect_find_by_code().returning(|_, _| Ok(None));
        links.expect_find_any_by_code().returning(|_| Ok(None));
        let reader = MockClickStatsReader::new();

        let svc = StatsService::new(Arc::new(reader), Arc::new(links));
        let err = svc
            .get_detailed_stats("missing", StatsFilter::new(0, 25))
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::NotFound { .. }));
    }

    #[tokio::test]
    async fn test_get_detailed_stats_reader_unavailable_propagates_503() {
        let mut links = MockLinkRepository::new();
        links
            .expect_find_by_code()
            .returning(|_, _| Ok(Some(link(1, "aaa"))));
        let mut reader = MockClickStatsReader::new();
        reader.expect_count_clicks().returning(|_, _| {
            Err(AppError::service_unavailable("down", serde_json::json!({})))
        });

        let filter = StatsFilter::new(0, 25).with_domain(Some(1));
        let svc = StatsService::new(Arc::new(reader), Arc::new(links));
        let err = svc.get_detailed_stats("aaa", filter).await.unwrap_err();
        assert!(matches!(err, AppError::ServiceUnavailable { .. }));
    }
}
```

- [ ] **Step 2: Add `find_any_by_code` to `LinkRepository`**

The detailed-stats path needs a code-only lookup. In `src/domain/repositories/link_repository.rs` add to the trait:

```rust
    /// Finds a link by code across all domains (used by stats when no domain filter).
    ///
    /// # Errors
    /// Returns [`AppError::Internal`] on database errors.
    async fn find_any_by_code(&self, code: &str) -> Result<Option<Link>, AppError>;
```

Implement it in `src/infrastructure/persistence/pg_link_repository.rs` mirroring the existing `find_by_code` but without the `domain_id` predicate (select the most recently created match). Use the same row→`Link` mapping as `find_by_code`. After adding the query, run `cargo sqlx prepare`.

- [ ] **Step 3: Remove the `StatsRepository` trait, keep the value types**

In `src/domain/repositories/stats_repository.rs` delete the `#[cfg_attr(test, mockall::automock)] pub trait StatsRepository { ... }` block (everything from `pub trait StatsRepository` to its closing brace) and the `use` lines it alone needed (`Click`, `NewClick`, `async_trait`). Keep `StatsFilter`, `LinkStats`, `DetailedStats`.

In `src/domain/repositories/mod.rs` change the stats export to only the types:

```rust
pub use stats_repository::{DetailedStats, LinkStats, StatsFilter};
```

and delete the `#[cfg(test)] pub use stats_repository::MockStatsRepository;` line.

- [ ] **Step 4: Remove `NewClick`**

In `src/domain/entities/click.rs` delete the `NewClick` struct and its test `test_new_click_creation`. In `src/domain/entities/mod.rs` remove any `NewClick` re-export.

- [ ] **Step 5: Update config (add new, remove old)**

In `src/config.rs`:
- Remove fields `click_queue_capacity` and `click_worker_concurrency`, their `from_env` parsing, their `validate` checks, and all references in `print_summary`, `test_config_validation`, `base_config`, and the two now-obsolete tests `test_validate_click_worker_concurrency_bounds` / the `click_queue_capacity` assertions (delete those tests; remove the field from `base_config`/`test_config_validation`).
- Add fields:

```rust
    pub kafka_brokers: Option<String>,
    pub kafka_clicks_topic: String,
    pub kafka_consumer_group: String,
    pub clickhouse_url: Option<String>,
    pub clickhouse_database: String,
    pub clickhouse_user: String,
    pub clickhouse_password: String,
    pub click_batch_size: usize,
    pub click_batch_flush_ms: u64,
```

- Parse them in `from_env`:

```rust
        let kafka_brokers = env::var("KAFKA_BROKERS").ok().filter(|s| !s.is_empty());
        let kafka_clicks_topic =
            env::var("KAFKA_CLICKS_TOPIC").unwrap_or_else(|_| "clicks".to_string());
        let kafka_consumer_group =
            env::var("KAFKA_CONSUMER_GROUP").unwrap_or_else(|_| "url_shortener_clicks".to_string());
        let clickhouse_url = env::var("CLICKHOUSE_URL").ok().filter(|s| !s.is_empty());
        let clickhouse_database =
            env::var("CLICKHOUSE_DATABASE").unwrap_or_else(|_| "url_shortener".to_string());
        let clickhouse_user = env::var("CLICKHOUSE_USER").unwrap_or_else(|_| "default".to_string());
        let clickhouse_password = env::var("CLICKHOUSE_PASSWORD").unwrap_or_default();
        let click_batch_size = env::var("CLICK_BATCH_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(500usize);
        let click_batch_flush_ms = env::var("CLICK_BATCH_FLUSH_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1000u64);
```

- Add validation in `validate`:

```rust
        if self.click_batch_size == 0 || self.click_batch_size > 100_000 {
            anyhow::bail!(
                "CLICK_BATCH_SIZE must be between 1 and 100000, got {}",
                self.click_batch_size
            );
        }
        if self.click_batch_flush_ms == 0 {
            anyhow::bail!("CLICK_BATCH_FLUSH_MS must be greater than 0");
        }
```

- Add the new fields to every `Config { .. }` literal in tests (`base_config`, `test_config_validation`).

- [ ] **Step 6: Update `AppState`**

In `src/state.rs`:
- Replace `use ... mpsc;` and the `click_sender` field with `pub click_publisher: Arc<dyn ClickPublisher>`.
- Change the `StatsService` field type to `Arc<StatsService<ClickHouseStatsReader, PgLinkRepository>>` ... but to avoid leaking concrete reader types and the `Unavailable` fallback, store the reader behind a trait object: change the field to `pub stats_service: Arc<StatsService<dyn ClickStatsReader, PgLinkRepository>>`. Since `StatsService` is generic over `R: ClickStatsReader`, switch its first param to `Arc<dyn ClickStatsReader>`:
  - In `stats_service.rs`, change the struct/impl to `StatsService<L: LinkRepository>` holding `reader: Arc<dyn ClickStatsReader>` (drop the `R` generic; trait object is simpler for the runtime fallback). Update `new(reader: Arc<dyn ClickStatsReader>, links: Arc<L>)`. Update the tests to pass `Arc::new(reader) as Arc<dyn ClickStatsReader>`.
- Update `AppState::new` signature: drop `click_sender`, add `click_publisher: Arc<dyn ClickPublisher>` and `stats_reader: Arc<dyn ClickStatsReader>`; build `StatsService::new(stats_reader, link_repo.clone())`. Remove the `stats_repo` parameter.

> This generic simplification (trait object reader) keeps `AppState` clean and supports the configured/unconfigured fallback uniformly.

- [ ] **Step 7: Update `redirect.rs` to publish via the publisher and read `link_id` from cache**

- Change the cache encoding helpers to include `link_id`:

```rust
/// Encodes `link_id` + URL with a redirect-type prefix for caching: `"{1|0}:{id}|{url}"`.
fn encode_cached_value(link_id: i64, url: &str, permanent: bool) -> String {
    let p = if permanent { PERMANENT_PREFIX } else { TEMPORARY_PREFIX };
    format!("{}{}|{}", p, link_id, url)
}

/// Parses a cached value into `(link_id, url, permanent)`.
/// Legacy entries without an id yield `link_id = 0` (click is skipped).
fn parse_cached_value(value: &str) -> (i64, String, bool) {
    let (permanent, rest) = if let Some(r) = value.strip_prefix(PERMANENT_PREFIX) {
        (true, r)
    } else if let Some(r) = value.strip_prefix(TEMPORARY_PREFIX) {
        (false, r)
    } else {
        (false, value)
    };
    match rest.split_once('|') {
        Some((id, url)) => (id.parse().unwrap_or(0), url.to_string(), permanent),
        None => (0, rest.to_string(), permanent),
    }
}
```

- In the handler, bind `(long_url, permanent, link_id)` from all three arms (HIT parses id from cache; MISS uses `link.id` and encodes it when caching; Err uses `link.id`).
- Replace the publish block with:

```rust
    if link_id > 0 {
        let event = ClickEvent::new(
            link_id,
            Some(addr.ip().to_string()),
            headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).map(String::from),
            headers.get(header::REFERER).and_then(|v| v.to_str().ok()).map(String::from),
            chrono::Utc::now(),
        );
        let publisher = state.click_publisher.clone();
        tokio::spawn(async move {
            let _ = publisher.publish(event).await;
        });
    }
```

- Update the unit tests in `redirect.rs` (if any test `encode_cached_value`/`parse_cached_value`) to the new `(link_id, url, permanent)` shape, e.g. assert round-trip preserves the id.

- [ ] **Step 8: Update health checks**

In `src/api/dto/health.rs` add `kafka: CheckStatus` and `clickhouse: CheckStatus` to `HealthChecks`, and remove `click_queue`.

In `src/api/handlers/health.rs`:
- Remove `check_click_queue`.
- Add `check_kafka(state)` and `check_clickhouse(state)` returning `CheckStatus` (using `state.kafka_health()` / `state.clickhouse_health()` — expose these via `AppState`, see below).
- Change the readiness logic so only the database is critical:

```rust
    let db_check = check_database(&state).await;
    let cache_check = check_cache(&state).await;
    let kafka_check = check_kafka(&state).await;
    let ch_check = check_clickhouse(&state).await;

    let critical_ok = db_check.status == "ok";
    let all_ok = critical_ok
        && cache_check.status == "ok"
        && kafka_check.status == "ok"
        && ch_check.status == "ok";

    let response = HealthResponse {
        status: if all_ok { "healthy" } else { "degraded" }.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        checks: HealthChecks {
            database: db_check,
            cache: cache_check,
            kafka: kafka_check,
            clickhouse: ch_check,
        },
    };

    if critical_ok {
        Ok(Json(response)) // 200 even when degraded
    } else {
        Err((StatusCode::SERVICE_UNAVAILABLE, Json(response)))
    }
```

To support the probes, add to `AppState` two optional handles: `pub kafka: Option<Arc<KafkaClickPublisher>>` and `pub clickhouse: Option<Arc<ReconnectingClickHouse>>`, and methods `pub async fn kafka_health(&self) -> bool` (true if `None` → not configured counts as "ok"/degraded? — treat unconfigured as degraded: return based on presence) — define: `check_kafka` reports `ok` when configured+reachable, `error` otherwise. Implement `kafka_health`/`clickhouse_health` accordingly (`None → false`). Keep the publisher trait object for publishing; the concrete handles are only for health/consumer.

- [ ] **Step 9: Rewrite `server.rs` wiring**

In `src/server.rs`:
- Remove the mpsc channel, `run_click_worker`, and the `PgStatsRepository`.
- After building the pool + repos, build infra:

```rust
    use crate::domain::repositories::{ClickPublisher, ClickStatsReader};
    use crate::infrastructure::messaging::{
        click_consumer::run_click_consumer, KafkaClickPublisher, NoopClickPublisher,
    };
    use crate::infrastructure::persistence::{
        ClickHouseConfig, ClickHouseStatsReader, ClickSink, ReconnectingClickHouse,
        UnavailableStatsReader,
    };
    use tokio_util::sync::CancellationToken;
    use std::time::Duration;

    // ClickHouse (read + sink) — optional.
    let clickhouse: Option<Arc<ReconnectingClickHouse>> = config.clickhouse_url.as_ref().map(|url| {
        Arc::new(ReconnectingClickHouse::new(ClickHouseConfig {
            url: url.clone(),
            database: config.clickhouse_database.clone(),
            user: config.clickhouse_user.clone(),
            password: config.clickhouse_password.clone(),
            retry_interval: Duration::from_secs(30),
        }))
    });
    let stats_reader: Arc<dyn ClickStatsReader> = match &clickhouse {
        Some(ch) => Arc::new(ClickHouseStatsReader::new(ch.clone())),
        None => Arc::new(UnavailableStatsReader),
    };

    // Kafka publisher — optional.
    let kafka: Option<Arc<KafkaClickPublisher>> = match &config.kafka_brokers {
        Some(brokers) => match KafkaClickPublisher::new(brokers, config.kafka_clicks_topic.clone()) {
            Ok(p) => Some(Arc::new(p)),
            Err(e) => {
                tracing::warn!(error = %e, "Kafka producer init failed; clicks will be dropped");
                None
            }
        },
        None => None,
    };
    let click_publisher: Arc<dyn ClickPublisher> = match &kafka {
        Some(p) => p.clone(),
        None => Arc::new(NoopClickPublisher),
    };

    // Spawn the consumer only when both Kafka and ClickHouse are configured.
    let shutdown_token = CancellationToken::new();
    let consumer_handle = match (&config.kafka_brokers, &clickhouse) {
        (Some(brokers), Some(ch)) => {
            let sink: Arc<dyn ClickSink> = ch.clone();
            let handle = tokio::spawn(run_click_consumer(
                brokers.clone(),
                config.kafka_consumer_group.clone(),
                config.kafka_clicks_topic.clone(),
                sink,
                config.click_batch_size,
                Duration::from_millis(config.click_batch_flush_ms),
                shutdown_token.clone(),
            ));
            Some(handle)
        }
        _ => {
            tracing::warn!("Kafka and/or ClickHouse not configured; click ingestion disabled");
            None
        }
    };
```

- Build `AppState::new(...)` with `click_publisher`, `stats_reader`, `kafka`, `clickhouse` (and remove the old `stats_repo`/`click_tx` args).
- After `axum::serve(...).await?`, trigger shutdown and join the consumer:

```rust
    shutdown_token.cancel();
    if let Some(handle) = consumer_handle {
        handle.await.ok();
    }
```

- [ ] **Step 10: Drop the PG clicks table + remove dead files**

Create `migrations/20260620000000_drop_link_clicks.sql`:

```sql
-- Clicks now live in ClickHouse; the Postgres table is no longer used.
DROP TABLE IF EXISTS link_clicks;
```

Delete files:

```bash
git rm src/domain/click_worker.rs src/infrastructure/persistence/pg_stats_repository.rs
```

In `src/domain/mod.rs` remove `pub mod click_worker;` and update the module docs (remove the click-worker flow section). In `src/infrastructure/persistence/mod.rs` remove `pub mod pg_stats_repository;` and the `PgStatsRepository` export. In `src/error.rs` `map_sqlx_error`, remove the now-dead `"link_clicks_link_id_fkey"` constraint arm (replace with the generic fallback).

- [ ] **Step 11: Refresh sqlx cache, build, and run the full suite**

```bash
cargo sqlx prepare
cargo build
cargo test
```

Expected: compiles; all unit tests pass. (Integration tests that depended on `link_clicks` or `MockStatsRepository` must be updated — see Task 11.)

- [ ] **Step 12: Commit**

```bash
cargo fmt && cargo clippy -- -D warnings
git add -A
git commit -m "refactor: move clicks to Kafka/ClickHouse, drop PG click storage"
```

---

## Task 10: Docker Compose, Dockerfile, ClickHouse schema, and `.env.example`

Make the new pipeline runnable locally and buildable in CI/Docker.

**Files:**
- Create: `docker/clickhouse/init/01_schema.sql`
- Modify: `docker-compose.yml`
- Modify: `Dockerfile`
- Modify: `.env.example`

**Interfaces:** none (infra only).

- [ ] **Step 1: ClickHouse schema**

Create `docker/clickhouse/init/01_schema.sql`:

```sql
CREATE DATABASE IF NOT EXISTS url_shortener;

CREATE TABLE IF NOT EXISTS url_shortener.clicks (
    link_id    UInt64,
    ip         Nullable(String),
    user_agent Nullable(String),
    referer    Nullable(String),
    clicked_at DateTime64(3, 'UTC')
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(clicked_at)
ORDER BY (link_id, clicked_at);
```

- [ ] **Step 2: docker-compose services**

In `docker-compose.yml`, add a `kafka` (KRaft, single node) and a `clickhouse` service to `services:`, both on `app_network`, e.g.:

```yaml
  kafka:
    image: bitnami/kafka:3.7
    container_name: url_shortener_kafka
    environment:
      KAFKA_CFG_NODE_ID: "0"
      KAFKA_CFG_PROCESS_ROLES: "controller,broker"
      KAFKA_CFG_CONTROLLER_QUORUM_VOTERS: "0@kafka:9093"
      KAFKA_CFG_LISTENERS: "PLAINTEXT://:9092,CONTROLLER://:9093"
      KAFKA_CFG_ADVERTISED_LISTENERS: "PLAINTEXT://kafka:9092"
      KAFKA_CFG_LISTENER_SECURITY_PROTOCOL_MAP: "CONTROLLER:PLAINTEXT,PLAINTEXT:PLAINTEXT"
      KAFKA_CFG_CONTROLLER_LISTENER_NAMES: "CONTROLLER"
      ALLOW_PLAINTEXT_LISTENER: "yes"
    ports:
      - "9092:9092"
    healthcheck:
      test: ["CMD-SHELL", "kafka-topics.sh --bootstrap-server localhost:9092 --list || exit 1"]
      interval: 10s
      timeout: 10s
      retries: 10
      start_period: 20s
    restart: unless-stopped
    networks:
      - app_network

  clickhouse:
    image: clickhouse/clickhouse-server:24-alpine
    container_name: url_shortener_clickhouse
    environment:
      CLICKHOUSE_DB: url_shortener
      CLICKHOUSE_USER: default
      CLICKHOUSE_PASSWORD: ""
      CLICKHOUSE_DEFAULT_ACCESS_MANAGEMENT: "1"
    ports:
      - "8123:8123"
    volumes:
      - clickhouse_data:/var/lib/clickhouse
      - ./docker/clickhouse/init:/docker-entrypoint-initdb.d
    healthcheck:
      test: ["CMD-SHELL", "wget -qO- http://localhost:8123/ping || exit 1"]
      interval: 10s
      timeout: 5s
      retries: 5
      start_period: 10s
    restart: unless-stopped
    networks:
      - app_network
```

Add `clickhouse_data:` to the `volumes:` section. In the `app` service `environment:` add:

```yaml
      KAFKA_BROKERS: kafka:9092
      KAFKA_CLICKS_TOPIC: clicks
      KAFKA_CONSUMER_GROUP: url_shortener_clicks
      CLICKHOUSE_URL: http://clickhouse:8123
      CLICKHOUSE_DATABASE: url_shortener
      CLICKHOUSE_USER: default
      CLICKHOUSE_PASSWORD: ""
      CLICK_BATCH_SIZE: "500"
      CLICK_BATCH_FLUSH_MS: "1000"
```

and add `kafka`/`clickhouse` to the app's `depends_on` (no hard `service_healthy` requirement — degraded mode tolerates them being down, but `service_started` is fine).

> The app auto-creates the topic on first publish only if the broker allows it; Bitnami Kafka has `KAFKA_CFG_AUTO_CREATE_TOPICS_ENABLE` on by default. If disabled in your image, add a one-shot topic-create step.

- [ ] **Step 3: Dockerfile build deps for librdkafka**

In `Dockerfile`, in the `chef` stage (so both `cook` and `build` have them), after `WORKDIR /app` add:

```dockerfile
RUN apt-get update && apt-get install -y --no-install-recommends \
    cmake g++ make \
    && rm -rf /var/lib/apt/lists/*
```

(librdkafka is statically linked via the `cmake-build` feature, so the `runtime` stage needs no new packages.)

- [ ] **Step 4: `.env.example`**

Append the new variables with comments:

```bash
# Kafka (click events). Leave KAFKA_BROKERS empty to disable click publishing (degraded).
KAFKA_BROKERS=localhost:9092
KAFKA_CLICKS_TOPIC=clicks
KAFKA_CONSUMER_GROUP=url_shortener_clicks

# ClickHouse (click analytics). Leave CLICKHOUSE_URL empty to disable stats (503).
CLICKHOUSE_URL=http://localhost:8123
CLICKHOUSE_DATABASE=url_shortener
CLICKHOUSE_USER=default
CLICKHOUSE_PASSWORD=

# Click ingestion batching
CLICK_BATCH_SIZE=500
CLICK_BATCH_FLUSH_MS=1000
```

Remove the obsolete `CLICK_QUEUE_CAPACITY` / `CLICK_WORKER_CONCURRENCY` lines if present.

- [ ] **Step 5: Verify the image builds**

Run: `docker build -t url-shortener:phase1 .`
Expected: build succeeds (librdkafka compiles in the builder stage).

- [ ] **Step 6: Commit**

```bash
git add docker/ docker-compose.yml Dockerfile .env.example
git commit -m "chore: add Kafka + ClickHouse services and build deps"
```

---

## Task 11: Update integration tests and docs

Existing integration tests reference the removed `link_clicks` table / `MockStatsRepository` / health `click_queue`. Bring them in line and document the new architecture.

**Files:**
- Modify: `tests/*` that touch stats/health/clicks (inspect with the grep below).
- Modify: `CLAUDE.md` (stack + architecture notes), and any project `README` analytics section.

**Interfaces:** none.

- [ ] **Step 1: Find the affected tests**

Run: `grep -rln "link_clicks\|MockStatsRepository\|StatsRepository\|click_queue\|click_sender\|record_click" tests/ src/`
Expected: a list of files. Work through each.

- [ ] **Step 2: Update health integration tests**

For any test asserting `/health` JSON, replace `click_queue` expectations with `kafka` + `clickhouse` fields, and assert that with Postgres up but Kafka/ClickHouse down the endpoint returns **200** with `status == "degraded"`. Add a test asserting **503** only when the database check fails (if a DB-down harness exists; otherwise document as manual).

- [ ] **Step 3: Update or remove click-recording integration tests**

Tests that inserted into `link_clicks` or called `record_click` no longer apply. Replace stats-reading assertions with ones that mock `ClickStatsReader` at the service layer (already covered by Task 9 unit tests), and delete DB-level click tests. For redirect tests, assert the redirect still succeeds and (optionally) that a publish is attempted via a stub `ClickPublisher`.

- [ ] **Step 4: Run the whole suite**

Run: `cargo test`
Expected: PASS. Then `cargo clippy --all-targets -- -D warnings` → clean.

- [ ] **Step 5: Update docs**

In `CLAUDE.md`, update the stack line to mention Kafka + ClickHouse for clicks, note that `link_clicks` is gone, and that clicks are eventually-consistent (batch + Kafka lag). Add a one-paragraph "Clicks pipeline" note pointing at the design doc.

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add -A
git commit -m "test: align integration tests and docs with Kafka/ClickHouse clicks"
```

---

## Task 12: End-to-end smoke verification (manual)

A non-coding gate to confirm the pipeline works against real services before calling Phase 1 done.

- [ ] **Step 1: Bring up infra**

Run: `docker compose up -d db redis kafka clickhouse`
Wait for healthy: `docker compose ps`.

- [ ] **Step 2: Run migrations and start the app**

```bash
sqlx migrate run
cargo run
```

Expected logs: "ClickHouse connected", "Click consumer started", no panics.

- [ ] **Step 3: Create a link, hit it, read stats**

```bash
# create (use a real API token / default domain per .env)
curl -s -XPOST localhost:3000/api/shorten -H "Authorization: Bearer $API_TOKEN" \
  -H 'content-type: application/json' -d '{"url":"https://example.com"}'
# follow the redirect a few times
curl -s -D- "localhost:3000/<code>" -H 'Host: <domain>' -o /dev/null
# read stats (allow a second for the batch flush)
sleep 2
curl -s "localhost:3000/api/stats/<code>" -H "Authorization: Bearer $API_TOKEN"
```

Expected: `total` reflects the clicks; `items` lists them with IP/UA.

- [ ] **Step 4: Verify degraded mode**

```bash
docker compose stop clickhouse
curl -s -o /dev/null -w "%{http_code}\n" "localhost:3000/<code>" -H 'Host: <domain>'   # 30x — redirect still works
curl -s -o /dev/null -w "%{http_code}\n" "localhost:3000/api/stats/<code>" -H "Authorization: Bearer $API_TOKEN"  # 503
curl -s localhost:3000/health | jq '.status'   # "degraded", HTTP 200
docker compose start clickhouse
```

Expected: redirects 30x throughout; stats 503 while down; `/health` degraded/200; clicks buffered in Kafka are inserted after ClickHouse returns.

- [ ] **Step 5: Record results**

Note any deviations in the design doc's "Risks" section. If all green, Phase 1 is complete. Phases 2 (bulk deactivation) and 3 (Sentry/APM/Grafana) are separate specs.

---

## Self-Review notes (addressed)

- **Spec coverage:** clicks→Kafka (Tasks 7,9), Rust consumer→ClickHouse (Tasks 5,6,8), drop PG clicks (Task 9), split ports (Task 4), `link_id` in cache (Task 9 step 7), cross-store merge (Task 9 step 1), degraded mode + health (Task 9 step 8), `ServiceUnavailable` (Task 2), config/env (Task 9 step 5, Task 10), docker/build (Task 10), tests (Tasks throughout + 11), e2e (Task 12). All design sections map to a task.
- **`StatsService` generics:** finalized as `StatsService<L: LinkRepository>` holding `Arc<dyn ClickStatsReader>` (Task 9 step 6) — tests and `AppState` use the trait-object reader consistently.
- **At-least-once:** offsets committed only after successful insert; failed batch re-buffered (Task 8 `flush_batch`).
- **Verification hooks:** crate-API uncertainties (clickhouse bind/fetch, rdkafka commit) are flagged with explicit "verify against version" notes at the relevant steps, with the pinned versions in Task 1.
