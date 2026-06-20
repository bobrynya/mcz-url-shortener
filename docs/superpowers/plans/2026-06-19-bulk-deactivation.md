# Bulk Link Deactivation + Domain Selector Unification — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `POST /links/batch-deactivate` and `POST /links/batch-restore` (bulk soft-delete / restore), and unify the management API's domain selector on a single explicit `domain_id` (i64).

**Architecture:** Clean Architecture is preserved — new bulk SQL in the `LinkRepository` trait (`domain/`) + `PgLinkRepository` (`infrastructure/`), orchestration in `LinkService` (`application/`), HTTP-only concerns in `api/handlers/links.rs` with DTOs in `api/dto/`. Bulk operations are single `UPDATE ... RETURNING` statements guarded by `deleted_at IS [NOT] NULL` for idempotency. Domain unification swaps name/`Host` resolution for `domain_id` across `/shorten`, single link `PATCH`/`DELETE`, and stats endpoints; the public redirect keeps `Host` resolution.

**Tech Stack:** Rust (edition 2024, MSRV 1.96), axum 0.8, sqlx (PostgreSQL, offline cache in `.sqlx/`), `#[async_trait]`, `mockall`, `validator`, `serde_with`, `axum-test` + `serial_test` for integration tests.

## Global Constraints

- **Deactivation = soft-delete:** bulk deactivate sets `deleted_at = now()`; bulk restore clears `deleted_at`. No new column; no DB migration.
- **`domain_id` (i64) is the single explicit domain selector** across the management API. Domain-name fields and `Host`-based resolution are removed from admin operations. The public redirect `GET /{code}` keeps `Host` resolution and is **not** touched.
- **Default-value rules:** mutating ops (`/shorten`, single `PATCH`/`DELETE /links/{code}`, batch-deactivate, batch-restore) with `domain_id` omitted ⇒ **default domain**. Stats filters (`GET /stats`, `GET /stats/{code}`) with `domain_id` omitted ⇒ **no domain filter**.
- **Batch limits:** `codes` is a non-empty array, **1..=1000** items (limit checked on the raw array, before de-duplication). Duplicates are de-duplicated keeping each code's **first occurrence**, which also defines `items` order.
- **Partial success:** batch endpoints return HTTP 200 with `{summary, items}` (mirroring `/shorten`); a missing/no-op code yields per-item `not_found`, never failing the request. Operations are idempotent.
- **`not_found` semantics:** deactivate — code absent OR already soft-deleted; restore — code absent OR already active.
- **Cache invalidation lives in the handler**, best-effort (`tracing::warn!` on error, never fail the request) — `LinkService` does not hold the cache.
- **Errors:** use `AppError`; no `unwrap()`/`expect()` outside tests. `cargo clippy -- -D warnings` and `cargo fmt --check` must pass.
- **Logging:** `tracing::{info,warn,error,debug}` — no `println!`.
- **sqlx offline cache:** after adding/altering any `sqlx::query!`, run `cargo sqlx prepare -- --all-targets` and commit the updated `.sqlx/`.
- **Commits:** conventional (`<type>: <subject>`), end the body with `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.

### Test environment note

Integration tests use `#[sqlx::test]`, which provisions an ephemeral database from `migrations/` and needs a reachable PostgreSQL via `DATABASE_URL`. The default domain seeded by migrations is **`s.example.com`** and is the row returned by `common::get_default_domain(&pool)`. The repository macros (`sqlx::query!`) require either a live `DATABASE_URL` or an up-to-date `.sqlx/` offline cache (`SQLX_OFFLINE=true`).

---

## File Structure

**Create:**
- `src/api/dto/batch_links.rs` — request/response DTOs for the two batch endpoints.
- `tests/handler_batch_links.rs` — integration tests for the batch endpoints.

**Modify:**
- `src/domain/repositories/link_repository.rs` — add `soft_delete_many`, `restore_many` to the trait.
- `src/infrastructure/persistence/pg_link_repository.rs` — implement both.
- `src/application/services/link_service.rs` — add `deactivate_links`, `restore_links`.
- `src/application/services/domain_service.rs` — add `get_domain_by_id`.
- `src/api/dto/mod.rs` — register `batch_links`.
- `src/api/dto/shorten.rs` — `UrlItem.domain: Option<String>` → `domain_id: Option<i64>`.
- `src/api/dto/update_link.rs` — add `domain_id: Option<i64>`.
- `src/api/dto/pagination.rs` — `StatsQueryParams.domain: Option<String>` → `domain_id: Option<i64>`.
- `src/api/handlers/links.rs` — add batch handlers; refactor `shorten`/`update`/`delete` to `domain_id`.
- `src/api/handlers/stats.rs` — use `domain_id` directly.
- `src/api/handlers/mod.rs` — export batch handlers.
- `src/api/routes.rs` — register batch routes.
- `tests/handler_links.rs`, `tests/handler_shorten.rs`, `tests/handler_stats.rs` — migrate to `domain_id`.
- `tests/repository_link.rs` — bulk repository tests.
- `README.md`, `CLAUDE.md` — documentation.
- `.sqlx/` — regenerated offline cache.

---

## Task 1: Repository — bulk soft-delete and restore

**Files:**
- Modify: `src/domain/repositories/link_repository.rs` (add two trait methods after `update`, around line 246)
- Modify: `src/infrastructure/persistence/pg_link_repository.rs` (add two impl methods after `update`, around line 290)
- Test: `tests/repository_link.rs` (append bulk tests)

**Interfaces:**
- Consumes: existing `LinkRepository` trait, `PgLinkRepository { pool: Arc<PgPool> }`, `AppError`.
- Produces:
  - `async fn soft_delete_many(&self, codes: &[String], domain_id: i64) -> Result<Vec<String>, AppError>` — deactivates active codes in the domain; returns the codes actually transitioned.
  - `async fn restore_many(&self, codes: &[String], domain_id: i64) -> Result<Vec<String>, AppError>` — restores soft-deleted codes in the domain; returns the codes actually transitioned.

- [ ] **Step 1: Write the failing repository tests**

Append to `tests/repository_link.rs`:

```rust
#[sqlx::test]
async fn test_soft_delete_many_transitions_only_active(pool: PgPool) {
    let repo = PgLinkRepository::new(Arc::new(pool.clone()));
    let domain_id = common::get_default_domain(&pool).await;

    common::create_test_link(&pool, "bd001", "https://a.com", domain_id).await;
    common::create_test_link(&pool, "bd002", "https://b.com", domain_id).await;
    common::create_deleted_link(&pool, "bd003", "https://c.com", domain_id).await;

    let codes = vec![
        "bd001".to_string(),
        "bd002".to_string(),
        "bd003".to_string(), // already deleted → not transitioned
        "bd404".to_string(), // missing → not transitioned
    ];
    let mut affected = repo.soft_delete_many(&codes, domain_id).await.unwrap();
    affected.sort();

    assert_eq!(affected, vec!["bd001".to_string(), "bd002".to_string()]);

    // Re-running is idempotent: nothing left to transition.
    let again = repo.soft_delete_many(&codes, domain_id).await.unwrap();
    assert!(again.is_empty());
}

#[sqlx::test]
async fn test_soft_delete_many_scoped_to_domain(pool: PgPool) {
    let repo = PgLinkRepository::new(Arc::new(pool.clone()));
    let default_id = common::get_default_domain(&pool).await;
    let other_id = common::create_test_domain(&pool, "other.example.com").await;

    common::create_test_link(&pool, "shared", "https://a.com", default_id).await;
    common::create_test_link(&pool, "shared", "https://b.com", other_id).await;

    let affected = repo
        .soft_delete_many(&["shared".to_string()], default_id)
        .await
        .unwrap();
    assert_eq!(affected, vec!["shared".to_string()]);

    // The other domain's "shared" link is untouched and still deactivatable.
    let other = repo
        .soft_delete_many(&["shared".to_string()], other_id)
        .await
        .unwrap();
    assert_eq!(other, vec!["shared".to_string()]);
}

#[sqlx::test]
async fn test_soft_delete_many_empty_input(pool: PgPool) {
    let repo = PgLinkRepository::new(Arc::new(pool.clone()));
    let domain_id = common::get_default_domain(&pool).await;
    let affected = repo.soft_delete_many(&[], domain_id).await.unwrap();
    assert!(affected.is_empty());
}

#[sqlx::test]
async fn test_restore_many_transitions_only_deleted(pool: PgPool) {
    let repo = PgLinkRepository::new(Arc::new(pool.clone()));
    let domain_id = common::get_default_domain(&pool).await;

    common::create_deleted_link(&pool, "rs001", "https://a.com", domain_id).await;
    common::create_deleted_link(&pool, "rs002", "https://b.com", domain_id).await;
    common::create_test_link(&pool, "rs003", "https://c.com", domain_id).await; // active → not transitioned

    let codes = vec![
        "rs001".to_string(),
        "rs002".to_string(),
        "rs003".to_string(),
        "rs404".to_string(),
    ];
    let mut affected = repo.restore_many(&codes, domain_id).await.unwrap();
    affected.sort();
    assert_eq!(affected, vec!["rs001".to_string(), "rs002".to_string()]);

    // Idempotent: already restored → nothing to transition.
    let again = repo.restore_many(&codes, domain_id).await.unwrap();
    assert!(again.is_empty());
}
```

Confirm the test file already has `mod common;`, `use sqlx::PgPool;`, `use std::sync::Arc;`, and `use url_shortener::infrastructure::persistence::PgLinkRepository;` at the top; add any that are missing.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test repository_link soft_delete_many`
Expected: FAIL — `no method named soft_delete_many found for struct PgLinkRepository`.

- [ ] **Step 3: Add the trait methods**

In `src/domain/repositories/link_repository.rs`, immediately after the `update` method (the last method in the trait, ending at line 246) and before the closing `}` of the trait, add:

```rust
    /// Deactivates (soft-deletes) the given codes within a domain in one
    /// statement. Only currently-active links are transitioned. Returns the
    /// codes that were actually transitioned, so callers can compute which
    /// inputs were `not_found` (absent or already deleted).
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Internal`] on database errors.
    async fn soft_delete_many(
        &self,
        codes: &[String],
        domain_id: i64,
    ) -> Result<Vec<String>, AppError>;

    /// Restores the given soft-deleted codes within a domain in one statement.
    /// Only currently-deleted links are transitioned. Returns the codes that
    /// were actually transitioned.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Internal`] on database errors.
    async fn restore_many(
        &self,
        codes: &[String],
        domain_id: i64,
    ) -> Result<Vec<String>, AppError>;
```

(`#[cfg_attr(test, mockall::automock)]` on the trait auto-generates the mock methods — no manual mock edits.)

- [ ] **Step 4: Implement in PgLinkRepository**

In `src/infrastructure/persistence/pg_link_repository.rs`, after the `update` method (ends at line 290) and before the final `}` closing the `impl LinkRepository` block, add:

```rust
    async fn soft_delete_many(
        &self,
        codes: &[String],
        domain_id: i64,
    ) -> Result<Vec<String>, AppError> {
        let rows = sqlx::query!(
            r#"
            UPDATE links
            SET deleted_at = now()
            WHERE domain_id = $1
              AND code = ANY($2)
              AND deleted_at IS NULL
            RETURNING code
            "#,
            domain_id,
            codes,
        )
        .fetch_all(self.pool.as_ref())
        .await?;

        Ok(rows.into_iter().map(|r| r.code).collect())
    }

    async fn restore_many(
        &self,
        codes: &[String],
        domain_id: i64,
    ) -> Result<Vec<String>, AppError> {
        let rows = sqlx::query!(
            r#"
            UPDATE links
            SET deleted_at = NULL
            WHERE domain_id = $1
              AND code = ANY($2)
              AND deleted_at IS NOT NULL
            RETURNING code
            "#,
            domain_id,
            codes,
        )
        .fetch_all(self.pool.as_ref())
        .await?;

        Ok(rows.into_iter().map(|r| r.code).collect())
    }
```

Note: sqlx binds `&[String]` to PostgreSQL `text[]` for `code = ANY($2)`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --test repository_link soft_delete_many` then `cargo test --test repository_link restore_many`
Expected: PASS (4 new tests).

- [ ] **Step 6: Regenerate the sqlx offline cache**

Run: `cargo sqlx prepare -- --all-targets`
Expected: new `.sqlx/query-*.json` files for the two `UPDATE ... RETURNING` queries.

- [ ] **Step 7: Lint and format**

Run: `cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add src/domain/repositories/link_repository.rs src/infrastructure/persistence/pg_link_repository.rs tests/repository_link.rs .sqlx
git commit -m "feat: bulk soft_delete_many/restore_many on link repository

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: Service layer — deactivate/restore links + domain lookup by id

**Files:**
- Modify: `src/application/services/link_service.rs` (add two methods after `update_link`, ~line 172; add unit tests in the existing `mod tests`)
- Modify: `src/application/services/domain_service.rs` (add `get_domain_by_id` after `get_default_domain`, ~line 93)

**Interfaces:**
- Consumes: `LinkRepository::{soft_delete_many, restore_many}` (Task 1); `DomainRepository::find_by_id`; `MockLinkRepository`, `MockDomainRepository`.
- Produces:
  - `LinkService::deactivate_links(&self, codes: Vec<String>, domain_id: i64) -> Result<Vec<String>, AppError>`
  - `LinkService::restore_links(&self, codes: Vec<String>, domain_id: i64) -> Result<Vec<String>, AppError>`
  - `DomainService::get_domain_by_id(&self, id: i64) -> Result<Domain, AppError>`

- [ ] **Step 1: Write the failing service unit tests**

In `src/application/services/link_service.rs`, inside `mod tests` (before the closing `}` at line 490), add:

```rust
    #[tokio::test]
    async fn test_deactivate_links_dedups_and_returns_affected() {
        let mut mock_link_repo = MockLinkRepository::new();
        let mock_domain_repo = MockDomainRepository::new();

        // Dedup keeps first occurrence; "a","b" only (the repeated "a" collapses).
        mock_link_repo
            .expect_soft_delete_many()
            .withf(|codes, domain_id| {
                *domain_id == 1 && codes == ["a".to_string(), "b".to_string()]
            })
            .times(1)
            .returning(|_, _| Ok(vec!["a".to_string()]));

        let service = LinkService::new(Arc::new(mock_link_repo), Arc::new(mock_domain_repo), true);

        let affected = service
            .deactivate_links(
                vec!["a".to_string(), "b".to_string(), "a".to_string()],
                1,
            )
            .await
            .unwrap();

        assert_eq!(affected, vec!["a".to_string()]);
    }

    #[tokio::test]
    async fn test_restore_links_delegates_to_repo() {
        let mut mock_link_repo = MockLinkRepository::new();
        let mock_domain_repo = MockDomainRepository::new();

        mock_link_repo
            .expect_restore_many()
            .withf(|codes, domain_id| *domain_id == 2 && codes == ["x".to_string()])
            .times(1)
            .returning(|_, _| Ok(vec!["x".to_string()]));

        let service = LinkService::new(Arc::new(mock_link_repo), Arc::new(mock_domain_repo), true);

        let affected = service.restore_links(vec!["x".to_string()], 2).await.unwrap();
        assert_eq!(affected, vec!["x".to_string()]);
    }
```

In `src/application/services/domain_service.rs`, add a `#[cfg(test)] mod tests` block at the end of the file (if none exists) — first check whether one already exists and append there instead:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::Domain;
    use crate::domain::repositories::MockDomainRepository;
    use chrono::Utc;

    fn sample_domain(id: i64) -> Domain {
        Domain::new(
            id,
            "s.example.com".to_string(),
            true,
            true,
            None,
            Utc::now(),
            Utc::now(),
            None,
        )
    }

    #[tokio::test]
    async fn test_get_domain_by_id_found() {
        let mut repo = MockDomainRepository::new();
        repo.expect_find_by_id()
            .withf(|id| *id == 7)
            .times(1)
            .returning(|_| Ok(Some(sample_domain(7))));

        let service = DomainService::new(Arc::new(repo));
        let domain = service.get_domain_by_id(7).await.unwrap();
        assert_eq!(domain.id, 7);
    }

    #[tokio::test]
    async fn test_get_domain_by_id_not_found() {
        let mut repo = MockDomainRepository::new();
        repo.expect_find_by_id()
            .times(1)
            .returning(|_| Ok(None));

        let service = DomainService::new(Arc::new(repo));
        let err = service.get_domain_by_id(99).await.unwrap_err();
        assert!(matches!(err, AppError::NotFound { .. }));
    }
}
```

If `domain_service.rs` already has a `mod tests`, only add the two `get_domain_by_id` tests and the `sample_domain` helper (skip duplicate imports).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib deactivate_links restore_links get_domain_by_id`
Expected: FAIL — methods `deactivate_links` / `restore_links` / `get_domain_by_id` not found.

- [ ] **Step 3: Implement the LinkService methods**

In `src/application/services/link_service.rs`, after `update_link` (ends ~line 172), add:

```rust
    /// Bulk-deactivates `codes` within `domain_id`. Input is de-duplicated
    /// (first occurrence kept). Returns the codes actually transitioned
    /// (were active, now soft-deleted).
    pub async fn deactivate_links(
        &self,
        codes: Vec<String>,
        domain_id: i64,
    ) -> Result<Vec<String>, AppError> {
        let unique = dedup_preserving_order(codes);
        self.link_repository
            .soft_delete_many(&unique, domain_id)
            .await
    }

    /// Bulk-restores `codes` within `domain_id`. Input is de-duplicated
    /// (first occurrence kept). Returns the codes actually transitioned
    /// (were soft-deleted, now active).
    pub async fn restore_links(
        &self,
        codes: Vec<String>,
        domain_id: i64,
    ) -> Result<Vec<String>, AppError> {
        let unique = dedup_preserving_order(codes);
        self.link_repository.restore_many(&unique, domain_id).await
    }
```

Add this free function below the `impl` block (after line 196, before `#[cfg(test)]`):

```rust
/// De-duplicates codes, preserving the order of each code's first occurrence.
fn dedup_preserving_order(codes: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    codes
        .into_iter()
        .filter(|c| seen.insert(c.clone()))
        .collect()
}
```

- [ ] **Step 4: Implement DomainService::get_domain_by_id**

First inspect `src/application/services/domain_service.rs` to confirm the field name holding the repository (it is constructed as `DomainService::new(repository)` and stored — match the existing field, e.g. `self.repository`). After `get_default_domain` (~line 93), add:

```rust
    /// Looks up a domain by its primary key.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::NotFound`] if no domain has this id.
    pub async fn get_domain_by_id(&self, id: i64) -> Result<Domain, AppError> {
        self.repository
            .find_by_id(id)
            .await?
            .ok_or_else(|| {
                AppError::not_found("Domain not found", serde_json::json!({ "domain_id": id }))
            })
    }
```

Ensure `Domain` is in scope (it is used by other methods) and `AppError` is imported. If `serde_json` is not already imported in the file, use a fully-qualified `serde_json::json!` as written.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib deactivate_links restore_links get_domain_by_id`
Expected: PASS (4 tests).

- [ ] **Step 6: Lint and format**

Run: `cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add src/application/services/link_service.rs src/application/services/domain_service.rs
git commit -m "feat: LinkService bulk deactivate/restore + DomainService get_domain_by_id

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: Batch endpoints — DTOs, handlers, routes

**Files:**
- Create: `src/api/dto/batch_links.rs`
- Modify: `src/api/dto/mod.rs` (register module)
- Modify: `src/api/handlers/links.rs` (add two handlers)
- Modify: `src/api/handlers/mod.rs` (export handlers)
- Modify: `src/api/routes.rs` (register routes)
- Test: `tests/handler_batch_links.rs` (new)

**Interfaces:**
- Consumes: `LinkService::{deactivate_links, restore_links}` and `DomainService::{get_default_domain, get_domain_by_id}` (Task 2); `AppState` fields `link_service`, `domain_service`, `cache`; `validator::Validate`.
- Produces: handlers `batch_deactivate_handler`, `batch_restore_handler`; DTOs `BatchLinksRequest`, `BatchLinkItem`, `BatchDeactivateSummary`, `BatchDeactivateResponse`, `BatchRestoreSummary`, `BatchRestoreResponse`.

- [ ] **Step 1: Write the failing integration tests**

Create `tests/handler_batch_links.rs`:

```rust
mod common;

use axum::{Router, routing::post};
use axum_test::TestServer;
use serde_json::json;
use sqlx::PgPool;
use url_shortener::api::handlers::{batch_deactivate_handler, batch_restore_handler};

fn make_server(pool: PgPool) -> TestServer {
    let state = common::create_test_state(pool);
    let app = Router::new()
        .route("/api/links/batch-deactivate", post(batch_deactivate_handler))
        .route("/api/links/batch-restore", post(batch_restore_handler))
        .with_state(state);
    TestServer::new(app).unwrap()
}

#[sqlx::test]
async fn test_batch_deactivate_partial(pool: PgPool) {
    let domain_id = common::get_default_domain(&pool).await;
    common::create_test_link(&pool, "bx001", "https://a.com", domain_id).await;
    common::create_test_link(&pool, "bx002", "https://b.com", domain_id).await;
    common::create_deleted_link(&pool, "bx003", "https://c.com", domain_id).await;

    let server = make_server(pool);
    let response = server
        .post("/api/links/batch-deactivate")
        .json(&json!({
            "codes": ["bx001", "bx002", "bx003", "bx404"],
            "domain_id": domain_id
        }))
        .await;

    response.assert_status_ok();
    let body = response.json::<serde_json::Value>();
    assert_eq!(body["summary"]["total"], 4);
    assert_eq!(body["summary"]["deactivated"], 2);
    assert_eq!(body["summary"]["not_found"], 2);
    // items preserve input order.
    assert_eq!(body["items"][0]["code"], "bx001");
    assert_eq!(body["items"][0]["status"], "deactivated");
    assert_eq!(body["items"][2]["code"], "bx003");
    assert_eq!(body["items"][2]["status"], "not_found");
}

#[sqlx::test]
async fn test_batch_deactivate_idempotent(pool: PgPool) {
    let domain_id = common::get_default_domain(&pool).await;
    common::create_test_link(&pool, "id001", "https://a.com", domain_id).await;

    let server = make_server(pool);
    let req = json!({ "codes": ["id001"], "domain_id": domain_id });

    server
        .post("/api/links/batch-deactivate")
        .json(&req)
        .await
        .assert_status_ok();

    let second = server.post("/api/links/batch-deactivate").json(&req).await;
    let body = second.json::<serde_json::Value>();
    assert_eq!(body["summary"]["deactivated"], 0);
    assert_eq!(body["summary"]["not_found"], 1);
}

#[sqlx::test]
async fn test_batch_deactivate_defaults_to_default_domain(pool: PgPool) {
    let domain_id = common::get_default_domain(&pool).await;
    common::create_test_link(&pool, "df001", "https://a.com", domain_id).await;

    let server = make_server(pool);
    // No domain_id → default domain.
    let response = server
        .post("/api/links/batch-deactivate")
        .json(&json!({ "codes": ["df001"] }))
        .await;

    response.assert_status_ok();
    let body = response.json::<serde_json::Value>();
    assert_eq!(body["summary"]["deactivated"], 1);
}

#[sqlx::test]
async fn test_batch_restore(pool: PgPool) {
    let domain_id = common::get_default_domain(&pool).await;
    common::create_deleted_link(&pool, "rb001", "https://a.com", domain_id).await;
    common::create_test_link(&pool, "rb002", "https://b.com", domain_id).await; // active → not_found on restore

    let server = make_server(pool);
    let response = server
        .post("/api/links/batch-restore")
        .json(&json!({ "codes": ["rb001", "rb002"], "domain_id": domain_id }))
        .await;

    response.assert_status_ok();
    let body = response.json::<serde_json::Value>();
    assert_eq!(body["summary"]["restored"], 1);
    assert_eq!(body["summary"]["not_found"], 1);
    assert_eq!(body["items"][0]["status"], "restored");
}

#[sqlx::test]
async fn test_batch_deactivate_rejects_empty(pool: PgPool) {
    let server = make_server(pool);
    let response = server
        .post("/api/links/batch-deactivate")
        .json(&json!({ "codes": [] }))
        .await;
    response.assert_status_bad_request();
}

#[sqlx::test]
async fn test_batch_deactivate_rejects_over_limit(pool: PgPool) {
    let server = make_server(pool);
    let codes: Vec<String> = (0..1001).map(|i| format!("c{i}")).collect();
    let response = server
        .post("/api/links/batch-deactivate")
        .json(&json!({ "codes": codes }))
        .await;
    response.assert_status_bad_request();
}

#[sqlx::test]
async fn test_batch_deactivate_unknown_domain_id(pool: PgPool) {
    let server = make_server(pool);
    let response = server
        .post("/api/links/batch-deactivate")
        .json(&json!({ "codes": ["x"], "domain_id": 999999 }))
        .await;
    response.assert_status_not_found();
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test handler_batch_links`
Expected: FAIL to compile — `batch_deactivate_handler` / `batch_restore_handler` not found.

- [ ] **Step 3: Create the batch DTOs**

Create `src/api/dto/batch_links.rs`:

```rust
//! DTOs for the bulk link deactivate/restore endpoints.

use serde::{Deserialize, Serialize};
use validator::Validate;

/// Request body for `POST /api/links/batch-deactivate` and
/// `POST /api/links/batch-restore`.
#[derive(Debug, Deserialize, Validate)]
pub struct BatchLinksRequest {
    /// Short codes to act on. 1..=1000 items; duplicates are de-duplicated.
    #[validate(length(min = 1, max = 1000, message = "codes must contain 1..=1000 items"))]
    pub codes: Vec<String>,

    /// Target domain. When omitted, the default domain is used.
    pub domain_id: Option<i64>,
}

/// Per-code outcome.
#[derive(Debug, Serialize)]
pub struct BatchLinkItem {
    pub code: String,
    /// "deactivated" | "restored" | "not_found".
    pub status: String,
}

/// Summary for batch-deactivate.
#[derive(Debug, Serialize)]
pub struct BatchDeactivateSummary {
    pub total: usize,
    pub deactivated: usize,
    pub not_found: usize,
}

/// Response for `POST /api/links/batch-deactivate`.
#[derive(Debug, Serialize)]
pub struct BatchDeactivateResponse {
    pub summary: BatchDeactivateSummary,
    pub items: Vec<BatchLinkItem>,
}

/// Summary for batch-restore.
#[derive(Debug, Serialize)]
pub struct BatchRestoreSummary {
    pub total: usize,
    pub restored: usize,
    pub not_found: usize,
}

/// Response for `POST /api/links/batch-restore`.
#[derive(Debug, Serialize)]
pub struct BatchRestoreResponse {
    pub summary: BatchRestoreSummary,
    pub items: Vec<BatchLinkItem>,
}
```

- [ ] **Step 4: Register the DTO module**

In `src/api/dto/mod.rs`, add (in the module's alphabetical/existing grouping):

```rust
pub mod batch_links;
```

- [ ] **Step 5: Implement the handlers**

In `src/api/handlers/links.rs`, add the import near the other DTO imports (top of file, with the `crate::api::dto::...` group):

```rust
use crate::api::dto::batch_links::{
    BatchDeactivateResponse, BatchDeactivateSummary, BatchLinkItem, BatchLinksRequest,
    BatchRestoreResponse, BatchRestoreSummary,
};
```

Add these handlers at the end of the file:

```rust
/// Resolves the target domain for a batch request: an explicit `domain_id` (404
/// if unknown), or the default domain when omitted.
async fn resolve_batch_domain(
    state: &AppState,
    domain_id: Option<i64>,
) -> Result<crate::domain::entities::Domain, AppError> {
    match domain_id {
        Some(id) => state.domain_service.get_domain_by_id(id).await,
        None => state.domain_service.get_default_domain().await,
    }
}

/// Builds per-code items in original (de-duplicated) input order, marking each
/// code `affected_status` when present in `affected`, else `not_found`.
fn build_batch_items(
    requested: &[String],
    affected: &[String],
    affected_status: &str,
) -> Vec<BatchLinkItem> {
    let affected_set: std::collections::HashSet<&String> = affected.iter().collect();
    let mut seen = std::collections::HashSet::new();
    requested
        .iter()
        .filter(|c| seen.insert((*c).clone()))
        .map(|code| BatchLinkItem {
            code: code.clone(),
            status: if affected_set.contains(code) {
                affected_status.to_string()
            } else {
                "not_found".to_string()
            },
        })
        .collect()
}

/// Bulk-deactivates (soft-deletes) short links.
///
/// `POST /api/links/batch-deactivate` — body `{ "codes": [...], "domain_id"?: i64 }`.
/// Returns HTTP 200 with a per-code summary; missing or already-deleted codes are
/// reported as `not_found`. Idempotent.
pub async fn batch_deactivate_handler(
    State(state): State<AppState>,
    Json(payload): Json<BatchLinksRequest>,
) -> Result<Json<BatchDeactivateResponse>, AppError> {
    payload.validate()?;
    let domain = resolve_batch_domain(&state, payload.domain_id).await?;

    let affected = state
        .link_service
        .deactivate_links(payload.codes.clone(), domain.id)
        .await?;

    for code in &affected {
        let cache_key = format!("{}:{}", domain.domain, code);
        if let Err(e) = state.cache.invalidate(&cache_key).await {
            tracing::warn!(error = ?e, cache_key, "Failed to invalidate cache after batch deactivate");
        }
    }

    let items = build_batch_items(&payload.codes, &affected, "deactivated");
    Ok(Json(BatchDeactivateResponse {
        summary: BatchDeactivateSummary {
            total: items.len(),
            deactivated: affected.len(),
            not_found: items.len() - affected.len(),
        },
        items,
    }))
}

/// Bulk-restores soft-deleted short links.
///
/// `POST /api/links/batch-restore` — body `{ "codes": [...], "domain_id"?: i64 }`.
/// Returns HTTP 200 with a per-code summary; missing or already-active codes are
/// reported as `not_found`. Idempotent.
pub async fn batch_restore_handler(
    State(state): State<AppState>,
    Json(payload): Json<BatchLinksRequest>,
) -> Result<Json<BatchRestoreResponse>, AppError> {
    payload.validate()?;
    let domain = resolve_batch_domain(&state, payload.domain_id).await?;

    let affected = state
        .link_service
        .restore_links(payload.codes.clone(), domain.id)
        .await?;

    for code in &affected {
        let cache_key = format!("{}:{}", domain.domain, code);
        if let Err(e) = state.cache.invalidate(&cache_key).await {
            tracing::warn!(error = ?e, cache_key, "Failed to invalidate cache after batch restore");
        }
    }

    let items = build_batch_items(&payload.codes, &affected, "restored");
    Ok(Json(BatchRestoreResponse {
        summary: BatchRestoreSummary {
            total: items.len(),
            restored: affected.len(),
            not_found: items.len() - affected.len(),
        },
        items,
    }))
}
```

Note: `total = items.len()` and `not_found = items.len() - affected.len()` are computed on the **de-duplicated** item list, so all three counts agree even when the request contains duplicates. `affected.len()` is always ≤ unique input length.

- [ ] **Step 6: Export the handlers**

In `src/api/handlers/mod.rs`, extend the existing line
`pub use links::{delete_link_handler, shorten_handler, update_link_handler};`
to:
```rust
pub use links::{
    batch_deactivate_handler, batch_restore_handler, delete_link_handler, shorten_handler,
    update_link_handler,
};
```

- [ ] **Step 7: Register the routes**

In `src/api/routes.rs`, add to the imports from `crate::api::handlers::{...}`: `batch_deactivate_handler, batch_restore_handler`. Add to the router (and to the doc-comment list of endpoints):

```rust
        .route("/links/batch-deactivate", post(batch_deactivate_handler))
        .route("/links/batch-restore", post(batch_restore_handler))
```

(`post` is already imported in `routes.rs`.)

- [ ] **Step 8: Run the integration tests to verify they pass**

Run: `cargo test --test handler_batch_links`
Expected: PASS (7 tests).

- [ ] **Step 9: Lint and format**

Run: `cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: clean.

- [ ] **Step 10: Commit**

```bash
git add src/api/dto/batch_links.rs src/api/dto/mod.rs src/api/handlers/links.rs src/api/handlers/mod.rs src/api/routes.rs tests/handler_batch_links.rs
git commit -m "feat: add batch-deactivate and batch-restore endpoints

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: Unify `/shorten` on `domain_id`

**Files:**
- Modify: `src/api/dto/shorten.rs` (UrlItem: `domain` → `domain_id`)
- Modify: `src/api/handlers/links.rs` (`process_single_url`)
- Test: `tests/handler_shorten.rs` (migrate to `domain_id`)

**Interfaces:**
- Consumes: `DomainService::{get_default_domain, get_domain_by_id}`; `LinkService::create_short_link_for_domain`.
- Produces: `/shorten` request items use `domain_id: Option<i64>` instead of `domain: Option<String>`.

- [ ] **Step 1: Update the failing tests first**

`tests/handler_shorten.rs` builds the `TestServer` inline in each test (no shared helper):

```rust
let state = common::create_test_state(pool);
let app = Router::new().route("/api/shorten", post(shorten_handler)).with_state(state);
let server = TestServer::new(app).unwrap();
```

If any test in this file sends a `"domain": "<name>"` field in a request item, replace it with `"domain_id": <id>` (resolve via `common::create_test_domain(&pool, name).await`; note `create_test_state` consumes the pool, so capture the id before building the state). Then add this explicit test, building the server inline as above:

```rust
#[sqlx::test]
async fn test_shorten_with_explicit_domain_id(pool: PgPool) {
    let domain_id = common::create_test_domain(&pool, "alt.example.com").await;
    let state = common::create_test_state(pool);
    let app = Router::new()
        .route("/api/shorten", post(shorten_handler))
        .with_state(state);
    let server = TestServer::new(app).unwrap();

    let response = server
        .post("/api/shorten")
        .json(&json!({ "urls": [{ "url": "https://example.com", "domain_id": domain_id }] }))
        .await;

    response.assert_status_ok();
    let body = response.json::<serde_json::Value>();
    assert_eq!(body["summary"]["successful"], 1);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test handler_shorten`
Expected: FAIL — deserialization/compile mismatch (`domain_id` not a field yet, or old `domain` assertions break).

- [ ] **Step 3: Update the DTO**

In `src/api/dto/shorten.rs`, in `struct UrlItem`, replace:

```rust
    /// Optional domain override (otherwise uses default domain).
    pub domain: Option<String>,
```

with:

```rust
    /// Optional target domain id (otherwise uses the default domain).
    pub domain_id: Option<i64>,
```

- [ ] **Step 4: Update the handler**

In `src/api/handlers/links.rs`, in `process_single_url`, replace the domain-resolution block:

```rust
    let domain = if let Some(domain_name) = item.domain {
        state.domain_service.get_domain(&domain_name).await?
    } else {
        state.domain_service.get_default_domain().await?
    };
```

with:

```rust
    let domain = if let Some(domain_id) = item.domain_id {
        state.domain_service.get_domain_by_id(domain_id).await?
    } else {
        state.domain_service.get_default_domain().await?
    };
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --test handler_shorten`
Expected: PASS.

- [ ] **Step 6: Lint and format**

Run: `cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add src/api/dto/shorten.rs src/api/handlers/links.rs tests/handler_shorten.rs
git commit -m "refactor: select domain by domain_id in /shorten

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 5: Unify single link `PATCH`/`DELETE` on `domain_id`

**Files:**
- Modify: `src/api/dto/update_link.rs` (add `domain_id`)
- Modify: `src/api/handlers/links.rs` (`update_link_handler`, `delete_link_handler`)
- Test: `tests/handler_links.rs` (migrate from `Host` to `domain_id`)

**Interfaces:**
- Consumes: `DomainService::{get_default_domain, get_domain_by_id}`.
- Produces: `PATCH /links/{code}` reads `domain_id` from the body (optional → default); `DELETE /links/{code}` reads `domain_id` from a query param (optional → default). `Host`-header resolution and `extract_domain_from_headers` import are removed from `links.rs`.

- [ ] **Step 1: Update the failing tests first**

In `tests/handler_links.rs`:
- Replace `make_server`'s doc comment about `Host` accordingly.
- For DELETE calls: replace `.delete("/api/links/{code}").add_header("Host", "s.example.com")` with `.delete(&format!("/api/links/{code}?domain_id={domain_id}"))` where `domain_id` is obtained via `common::get_default_domain(&pool).await`. Where the test currently asserts a default-domain happy path, also keep one variant that omits `domain_id` to exercise the default:

```rust
#[sqlx::test]
async fn test_delete_link_defaults_to_default_domain(pool: PgPool) {
    let domain_id = common::get_default_domain(&pool).await;
    common::create_test_link(&pool, "del010", "https://example.com", domain_id).await;

    let server = make_server(pool);
    // No domain_id → default domain.
    server
        .delete("/api/links/del010")
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);
}
```

- For PATCH calls: drop `.add_header("Host", ...)` and add `"domain_id": domain_id` into the JSON body (resolve `domain_id` with `common::get_default_domain(&pool).await`). Example for the URL-update test:

```rust
    let response = server
        .patch("/api/links/upd001")
        .json(&json!({ "domain_id": domain_id, "url": "https://new.com" }))
        .await;
```

- Delete the `test_delete_link_missing_host_header` test (Host is no longer consulted). Replace it with an unknown-domain test:

```rust
#[sqlx::test]
async fn test_delete_link_unknown_domain_id(pool: PgPool) {
    let server = make_server(pool);
    let response = server.delete("/api/links/whatever?domain_id=999999").await;
    response.assert_status_not_found();
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test handler_links`
Expected: FAIL to compile / assertions fail against the new selector.

- [ ] **Step 3: Add `domain_id` to the update DTO**

In `src/api/dto/update_link.rs`, add to `struct UpdateLinkRequest`:

```rust
    /// Target domain id. When omitted, the default domain is used.
    pub domain_id: Option<i64>,
```

- [ ] **Step 4: Add a query DTO for delete**

In `src/api/handlers/links.rs`, add near the top (after imports), a small query struct:

```rust
/// Query parameters for `DELETE /api/links/{code}`.
#[derive(Debug, serde::Deserialize)]
pub struct DeleteLinkQuery {
    /// Target domain id. When omitted, the default domain is used.
    pub domain_id: Option<i64>,
}
```

- [ ] **Step 5: Refactor `update_link_handler`**

Replace the domain resolution in `update_link_handler`:

```rust
    let domain = extract_domain_from_headers(&headers)?;
    let domain_entity = state.domain_service.get_domain(&domain).await?;
```

with:

```rust
    let domain_entity = match payload.domain_id {
        Some(id) => state.domain_service.get_domain_by_id(id).await?,
        None => state.domain_service.get_default_domain().await?,
    };
    let domain = domain_entity.domain.clone();
```

Remove the `headers: HeaderMap` parameter from `update_link_handler`'s signature (it is no longer used). The later `state.link_service.update_link(&code, domain_entity.id, patch)`, the `cache_key = format!("{}:{}", domain, code)`, and `get_short_url(&domain, ...)` calls keep working with the local `domain` string.

- [ ] **Step 6: Refactor `delete_link_handler`**

Change the signature to take the query instead of headers:

```rust
pub async fn delete_link_handler(
    Path(code): Path<String>,
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<DeleteLinkQuery>,
) -> Result<StatusCode, AppError> {
    let domain_entity = match query.domain_id {
        Some(id) => state.domain_service.get_domain_by_id(id).await?,
        None => state.domain_service.get_default_domain().await?,
    };
    let domain = domain_entity.domain.clone();

    let deleted = state
        .link_service
        .soft_delete_link(&code, domain_entity.id)
        .await?;

    if !deleted {
        return Err(AppError::not_found(
            "Link not found or already deleted",
            json!({ "code": code }),
        ));
    }

    let cache_key = format!("{}:{}", domain, code);
    if let Err(e) = state.cache.invalidate(&cache_key).await {
        tracing::warn!(error = ?e, cache_key, "Failed to invalidate cache after delete");
    }

    Ok(StatusCode::NO_CONTENT)
}
```

- [ ] **Step 7: Remove the now-unused import**

In `src/api/handlers/links.rs`, remove `use crate::utils::extract_domain::extract_domain_from_headers;` and remove `HeaderMap` from the `axum::http` import if no other handler in the file uses it. (The redirect handler keeps its own import — do not touch `redirect.rs`.)

- [ ] **Step 8: Run the tests to verify they pass**

Run: `cargo test --test handler_links`
Expected: PASS.

- [ ] **Step 9: Lint and format**

Run: `cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: clean (no unused-import warnings).

- [ ] **Step 10: Commit**

```bash
git add src/api/dto/update_link.rs src/api/handlers/links.rs tests/handler_links.rs
git commit -m "refactor: select domain by domain_id in single link PATCH/DELETE

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 6: Unify stats endpoints on `domain_id`

**Files:**
- Modify: `src/api/dto/pagination.rs` (`StatsQueryParams.domain` → `domain_id`)
- Modify: `src/api/handlers/stats.rs` (both handlers)
- Test: `tests/handler_stats.rs` (migrate from `domain` to `domain_id`)

**Interfaces:**
- Consumes: `StatsFilter::with_domain(Option<i64>)`.
- Produces: `GET /stats` and `GET /stats/{code}` accept `?domain_id=<i64>` (optional → no filter) instead of `?domain=<name>`.

- [ ] **Step 1: Update the failing tests first**

In `tests/handler_stats.rs`, replace query usages `?domain=<name>` / `"domain"` params with `?domain_id=<id>` (resolve via `common::get_default_domain` / `common::create_test_domain`). Keep at least one test that omits `domain_id` to confirm the unfiltered path still returns results.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test handler_stats`
Expected: FAIL — `domain` no longer the param / assertions mismatch.

- [ ] **Step 3: Update the query DTO**

In `src/api/dto/pagination.rs`, in `struct StatsQueryParams`, replace:

```rust
    pub domain: Option<String>,
```

with:

```rust
    #[serde_as(as = "Option<DisplayFromStr>")]
    #[serde(default)]
    pub domain_id: Option<i64>,
```

`StatsQueryParams` must carry `#[serde_as]` for `DisplayFromStr` to apply. Add `#[serde_as]` directly above `#[derive(Debug, Deserialize)]` on `StatsQueryParams` (the `serde_with::{DisplayFromStr, serde_as}` import already exists in this file). `DisplayFromStr` is required because the struct flattens its sub-structs, so query values arrive as strings (same reason `page`/`page_size` use it).

- [ ] **Step 4: Update the stats handlers**

In `src/api/handlers/stats.rs`, in **both** `stats_list_handler` and `stats_handler`, replace:

```rust
    let domain_id = if let Some(domain_name) = &params.domain {
        let domain = state.domain_service.get_domain(domain_name).await?;
        Some(domain.id)
    } else {
        None
    };
```

with:

```rust
    let domain_id = params.domain_id;
```

Update the two doc-comment lines `- `domain` (optional): Filter by domain name` to `- `domain_id` (optional): Filter by domain id`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --test handler_stats`
Expected: PASS.

- [ ] **Step 6: Lint and format**

Run: `cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add src/api/dto/pagination.rs src/api/handlers/stats.rs tests/handler_stats.rs
git commit -m "refactor: filter stats by domain_id

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 7: Documentation + full verification

**Files:**
- Modify: `README.md`
- Modify: `CLAUDE.md`

**Interfaces:**
- Consumes: the final API surface from Tasks 1–6.
- Produces: user-facing docs for the new endpoints and the `domain_id` convention.

- [ ] **Step 1: Document the new endpoints in README.md**

Add an API section documenting:
- `POST /api/links/batch-deactivate` and `POST /api/links/batch-restore`: request `{ "codes": [...], "domain_id"?: i64 }` (1..=1000 codes, dedup), response `{summary, items}` with `deactivated`/`restored` + `not_found`, partial-success HTTP 200, idempotent. Include a `curl` example and a sample response (mirror the spec's examples).
- The unified `domain_id` selector across the management API, with a **breaking-change** note: `/shorten` uses `domain_id` (not `domain` name); single `PATCH`/`DELETE /links/{code}` use `domain_id` (body / `?domain_id=` query) instead of the `Host` header; `GET /stats` and `GET /stats/{code}` use `?domain_id=` instead of `?domain=`. State that the public redirect still resolves by `Host`.

- [ ] **Step 2: Update CLAUDE.md**

Under the API/architecture notes, add a short paragraph: the management API selects a domain by explicit `domain_id` (default domain when omitted for mutations; no filter when omitted for stats); the public redirect uses `Host`. List the two new batch endpoints.

- [ ] **Step 3: Full build, lint, format, and test sweep**

Run:
```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```
Expected: all pass. If any `.sqlx` drift remains, run `cargo sqlx prepare -- --all-targets` and include it in the commit.

- [ ] **Step 4: Verify offline build**

Run: `SQLX_OFFLINE=true cargo check`
Expected: clean — confirms `.sqlx/` covers the new queries.

- [ ] **Step 5: Commit**

```bash
git add README.md CLAUDE.md .sqlx
git commit -m "docs: document batch link endpoints and domain_id selector

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Notes for the executor

- **Order matters:** Task 1 → 2 → 3 deliver the feature; Tasks 4–6 are the breaking unification and are independent of each other (any order) but all depend on Task 2's `get_domain_by_id`. Task 7 is last.
- **Do not touch** `src/api/handlers/redirect.rs` or `src/utils/extract_domain.rs` — the public redirect keeps `Host` resolution (the function stays; only its import in `links.rs` is removed).
- **No DB migration** is added — deactivation reuses `deleted_at`.
- After Task 1, the `.sqlx/` cache must be regenerated or later `SQLX_OFFLINE` builds fail.
