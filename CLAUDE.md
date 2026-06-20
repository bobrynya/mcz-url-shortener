# URL Shortener — Claude Instructions

## Project

Rust URL shortener service. Stack: **axum 0.8 + PostgreSQL + Redis**, with
**Kafka + ClickHouse** for click analytics.
Clean Architecture: `domain → application → infrastructure → api/web`.
Rust edition 2024, MSRV 1.96 (pinned via `rust-toolchain.toml`).

### Clicks pipeline

Clicks are **not** stored in PostgreSQL (the `link_clicks` table is gone). On
redirect the handler resolves `link_id` and publishes a `ClickEvent` to Kafka
fire-and-forget (`ClickPublisher`, falls back to a no-op when Kafka is absent).
A background consumer batches events into ClickHouse, and the stats endpoints
read counts from ClickHouse via `ClickStatsReader` (link metadata still comes
from Postgres). Click counts are therefore **eventually consistent** — subject
to Kafka delivery lag plus the consumer's batch-flush interval — so tests and
callers must not expect a click to be queryable immediately after a redirect.
Kafka and ClickHouse are both **optional/non-critical**: when unconfigured,
clicks are dropped and stats reads return 503. See the design doc:
`docs/superpowers/specs/2026-06-19-clicks-kafka-clickhouse-design.md`.

## Commands

```bash
# Build & check
cargo check                          # fast type check
cargo build                          # debug build
cargo build --release                # release build
cargo clippy -- -D warnings          # lint (must pass clean)
cargo fmt --check                    # formatting check

# Test
cargo test                           # all tests
cargo test -- --nocapture            # with stdout
cargo test <name>                    # specific test
cargo tarpaulin --out Html           # coverage report → coverage/

# Database
sqlx migrate run                     # apply migrations
sqlx migrate revert                  # rollback last migration
sqlx prepare                         # update .sqlx/ offline query cache (run after schema changes)

# Assets
./tailwindcss -i static/input.css -o static/output.css --minify

# Run
cargo run                            # default binary (url-shortener)
cargo run --bin admin                # admin CLI tool
```

## Code Conventions

- **Errors**: use `AppError` (`src/error.rs`) with `thiserror`. Never `unwrap()` in non-test code.
- **Async traits**: use `#[async_trait]`.
- **Logging**: `tracing::{info, warn, error, debug}` — no `println!` in production code.
- **Formatting**: `rustfmt` defaults — run `cargo fmt` before committing.
- **Clippy**: all warnings are errors (`-D warnings`). Fix all lints.
- **No `unwrap()`/`expect()`** outside of tests, const initialization, or explicitly documented panic-safe contexts.
- **Imports**: group `std` → external crates → internal (`crate::`) with blank lines between groups.

## Architecture Rules

- Repository traits live in `domain/` — implementations in `infrastructure/`.
- Handlers in `api/handlers/` must not contain business logic — delegate to services.
- `AppState` is `Arc`-wrapped in `state.rs`; never clone the pool directly in handlers.
- New API endpoints go in `api/handlers/` with a corresponding route registered in `api/routes.rs`.
- Web (HTML) endpoints go in `web/` with routes in `web/routes.rs`.

### Domain Selector Convention

The management API selects a domain by explicit `domain_id` (i64, primary key from the `domains` table):
- **Mutating operations** (`POST /api/shorten`, `PATCH /api/links/{code}`, `DELETE /api/links/{code}`, `POST /api/links/batch-deactivate`, `POST /api/links/batch-restore`): `domain_id` omitted → default domain.
- **Stats filters** (`GET /api/stats`, `GET /api/stats/{code}`): `domain_id` omitted → no domain filter (all domains / cross-domain lookup).
- The **public redirect** (`GET /{code}`) continues to resolve the domain from the `Host` request header — unchanged.

Two bulk endpoints (both in `api/handlers/links.rs`, registered in `api/routes.rs`):
- `POST /api/links/batch-deactivate` — soft-deletes up to 1000 codes; partial success, idempotent.
- `POST /api/links/batch-restore` — restores up to 1000 soft-deleted codes; partial success, idempotent.
Request: `{ "codes": [...], "domain_id"?: i64 }`. Response: `{ "summary": { "total", "deactivated"|"restored", "not_found" }, "items": [{ "code", "status" }] }`.

## Testing

- **Unit tests**: in-module with `mockall` mocks (`MockLinkRepository` etc.).
- **Integration tests**: `tests/` directory, use `axum-test` TestServer + `serial_test` for env-var isolation.
- **Avoid** hitting a real DB in unit tests — use mocks.
- New public functions need at least one test.

## Database

- Run `cargo sqlx prepare -- --all-targets` after any `sqlx::query!` / schema change to keep `.sqlx/` in sync (the `--all-targets` flag also caches the queries used by integration tests under `tests/`).
- Migration files live in `migrations/` — always add new migrations, never modify existing ones.
- Schema summary: `links`, `domains`, `api_tokens` — see `migrations/` for details. The `link_clicks` table was dropped (migration `20260620000000_drop_link_clicks.sql`); clicks now live in ClickHouse (see the Clicks pipeline note above).

## Git

- Commit messages: imperative mood, `<type>: <subject>` (e.g. `feat: add click export endpoint`).
- Types: `feat`, `fix`, `refactor`, `test`, `chore`, `docs`.
- Never force-push, never skip hooks.
- `git push` requires explicit user confirmation — ask before pushing.

## Environment

Copy `.env.example` to `.env` and fill in `DATABASE_URL`, `REDIS_URL`, `API_TOKEN` etc.
Use `docker compose up -d` to start Postgres + Redis locally.
