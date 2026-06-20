# Phase 2: Bulk Link Deactivation + Domain Selector Unification — Design

> **Status:** Approved for planning.
> **Phase:** 2 of the post-Phase-1 roadmap (Phase 1 = clicks on Kafka + ClickHouse).
> **Predecessor spec:** `2026-06-19-clicks-kafka-clickhouse-design.md`.

## Goal

Add bulk deactivation and bulk restoration of short links, and unify how the
management API selects a domain: a single explicit `domain_id` (i64) replaces
the current mix of domain-name body fields, domain-name query params, and
`Host`-header resolution.

## Motivation

The Python reference (`mcz-url-shortener`) exposes
`POST /api/v1/urls/batch-deactivate`. Our admin tooling needs the same: deactivate
(and re-activate) many links in one call instead of N single requests.

While adding it, we resolve a pre-existing inconsistency. A domain is currently
referenced three different ways:

| Where | Current domain selector |
|---|---|
| `/domains/{id}` (management) | `id` (i64) in path; `GET /domains` returns `id` |
| `POST /shorten` (batch create) | `domain` — name (string), optional → default |
| `PATCH` / `DELETE /links/{code}` | `Host` request header |
| `GET /stats`, `GET /stats/{code}` | `domain` — name (string), query param, optional |

`domain_id` is already the public identifier of the **domains** resource, so
standardizing on it removes the inconsistency rather than introducing a foreign
concept.

## Scope Decisions (locked)

1. **Deactivation = soft-delete.** Bulk deactivate sets `deleted_at = now()`;
   bulk restore clears `deleted_at`. No new column; the Phase-1 data model is
   unchanged. This reuses the exact semantics of the existing single-link
   `DELETE /links/{code}` and `PATCH {restore: true}`.
2. **Two endpoints:** `POST /links/batch-deactivate` and
   `POST /links/batch-restore`.
3. **Partial success, HTTP 200**, mirroring the `/shorten` batch contract
   (`{summary, items[]}`). A missing/no-op code does not fail the request.
4. **Full domain-selector unification.** `domain_id` (i64) becomes the single
   explicit domain selector across the management API. Domain-name fields and
   `Host`-based resolution are removed from admin operations. The public
   redirect `GET /{code}` keeps `Host` resolution — it is host-bound by design.
5. **Breaking changes are acceptable.** The product is operated by its own
   authors; there is no external client contract to preserve. No deprecation
   window — the old selectors are replaced outright.

## Non-Goals

- No separate `is_active` boolean column (deactivation stays modeled as
  soft-delete).
- No change to the public redirect's `Host`-based domain resolution.
- No nested `/domains/{id}/links/...` route restructure — routes stay flat with
  `domain_id` as a field/query selector.
- No change to authentication, the clicks pipeline, or observability (Phase 3).

## API Surface

### New: `POST /links/batch-deactivate`

Request:

```json
{ "codes": ["abc123", "xyz456", "missing"], "domain_id": 1 }
```

- `codes`: non-empty array, **1–1000** items. Duplicates are de-duplicated
  before processing, keeping each code's **first occurrence** to define the
  `items` order; the 1000 limit is checked on the raw array before dedup.
- `domain_id`: **optional**. When omitted, the default domain is used (the
  uniform rule for mutating operations).

Response — HTTP 200:

```json
{
  "summary": { "total": 2, "deactivated": 2, "not_found": 0 },
  "items": [
    { "code": "abc123", "status": "deactivated" },
    { "code": "xyz456", "status": "deactivated" }
  ]
}
```

- `summary.total` = number of unique codes processed (after de-duplication).
- `status` is one of `deactivated` | `not_found`.
- **`not_found`** = no link in the deactivatable state for `(code, domain_id)`:
  the code does not exist **or** is already soft-deleted. This matches the
  existing single-link `soft_delete`, which returns `false` in both cases.
- **Idempotent:** re-running a deactivation yields `not_found` for codes already
  deactivated.

### New: `POST /links/batch-restore`

Identical request shape. Response uses `restored`/`not_found`:

```json
{
  "summary": { "total": 3, "restored": 1, "not_found": 2 },
  "items": [
    { "code": "abc123", "status": "restored" },
    { "code": "xyz456", "status": "not_found" },
    { "code": "missing", "status": "not_found" }
  ]
}
```

- `status` is one of `restored` | `not_found`.
- **`not_found`** for restore = the code does not exist **or** is already active
  (not soft-deleted). Symmetric to deactivate; idempotent.

### Changed: domain selector unification (breaking)

| Endpoint | Was | Becomes |
|---|---|---|
| `POST /shorten` | per-item `domain` (name) | per-item `domain_id` (i64, optional → default) |
| `PATCH /links/{code}` | `Host` header | body `domain_id` (i64, optional → default) |
| `DELETE /links/{code}` | `Host` header | query `?domain_id=` (i64, optional → default) |
| `GET /stats` | query `?domain=` (name) | query `?domain_id=` (i64, optional → all domains) |
| `GET /stats/{code}` | query `?domain=` (name) | query `?domain_id=` (i64, optional → cross-domain) |

Default-value rules:

- **Mutating operations** (`/shorten`, `PATCH`/`DELETE /links/{code}`,
  batch-deactivate, batch-restore): `domain_id` omitted ⇒ **default domain**.
- **Stats filters** (`GET /stats`, `GET /stats/{code}`): `domain_id` omitted ⇒
  **no domain filter** (all domains / cross-domain lookup), preserving today's
  behavior when `domain` was absent.

`extract_domain_from_headers` remains in use **only** by the redirect handler;
its import is removed from `api/handlers/links.rs`.

## Architecture & Components

Clean Architecture layering is preserved: repository traits in `domain/`,
implementations in `infrastructure/`, orchestration in `application/services/`,
HTTP-only concerns in `api/handlers/`.

### Domain layer — `domain/repositories/link_repository.rs`

Two new trait methods, each a single bulk SQL `UPDATE ... RETURNING`:

```rust
/// Deactivates the given codes within a domain in one statement. Returns the
/// codes actually transitioned (were active, now soft-deleted), so the caller
/// can compute not_found = input_unique − returned.
async fn soft_delete_many(
    &self,
    codes: &[String],
    domain_id: i64,
) -> Result<Vec<String>, AppError>;

/// Restores the given codes within a domain in one statement. Returns the codes
/// actually transitioned (were soft-deleted, now active).
async fn restore_many(
    &self,
    codes: &[String],
    domain_id: i64,
) -> Result<Vec<String>, AppError>;
```

SQL (PostgreSQL, `code = ANY($2)` over a `text[]` bind):

```sql
-- deactivate
UPDATE links
   SET deleted_at = now()
 WHERE domain_id = $1
   AND code = ANY($2)
   AND deleted_at IS NULL
RETURNING code;

-- restore
UPDATE links
   SET deleted_at = NULL
 WHERE domain_id = $1
   AND code = ANY($2)
   AND deleted_at IS NOT NULL
RETURNING code;
```

The `deleted_at IS [NOT] NULL` guard makes each operation idempotent and lets
`RETURNING` report exactly the transitioned rows.

### Application layer

`application/services/link_service.rs` — two methods:

```rust
/// De-duplicates `codes`, deactivates them in `domain_id`, and returns the
/// affected codes (those actually transitioned).
pub async fn deactivate_links(
    &self,
    codes: Vec<String>,
    domain_id: i64,
) -> Result<Vec<String>, AppError>;

pub async fn restore_links(
    &self,
    codes: Vec<String>,
    domain_id: i64,
) -> Result<Vec<String>, AppError>;
```

- De-duplicate the input codes before calling the repository.
- Return the affected codes; the handler computes `not_found`, builds the
  per-item response, and performs cache invalidation.
- **Cache invalidation lives in the handler**, not the service — matching the
  existing single-link `update`/`delete` handlers, where `LinkService` does not
  hold the cache and `state.cache.invalidate("{domain}:{code}")` is called from
  the handler. The batch handler invalidates each affected code best-effort
  (`tracing::warn!` on error, never failing the request).

`application/services/domain_service.rs` — add:

```rust
/// Looks up a domain by its primary key. Returns AppError::NotFound if absent.
pub async fn get_domain_by_id(&self, id: i64) -> Result<Domain, AppError>;
```

Wraps the existing `DomainRepository::find_by_id`. Used wherever a handler now
receives `domain_id` and needs the `Domain` entity (its name for cache keys and
`short_url`, and existence validation → 404).

### API layer

`api/dto/batch_links.rs` (new):

```rust
#[derive(Debug, Deserialize, Validate)]
pub struct BatchLinksRequest {
    #[validate(length(min = 1, max = 1000, message = "codes must contain 1..=1000 items"))]
    pub codes: Vec<String>,
    pub domain_id: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct BatchLinkItem {
    pub code: String,
    pub status: String, // "deactivated" | "restored" | "not_found"
}

#[derive(Debug, Serialize)]
pub struct BatchDeactivateSummary {
    pub total: usize,
    pub deactivated: usize,
    pub not_found: usize,
}

#[derive(Debug, Serialize)]
pub struct BatchRestoreSummary {
    pub total: usize,
    pub restored: usize,
    pub not_found: usize,
}
```

(`deactivated`/`restored` keys differ per endpoint; two summary structs keep the
JSON keys explicit rather than a generic `affected`.)

`api/handlers/links.rs`:

- `batch_deactivate_handler`, `batch_restore_handler` — resolve `domain_id`
  (or default domain) → `Domain`; call the service; build `{summary, items}`
  preserving **input order** for `items` and reporting `not_found` for codes the
  service did not return.
- `shorten_handler` / `process_single_url` — switch `item.domain` (name) to
  `item.domain_id` (optional → `get_default_domain`, else `get_domain_by_id`).
- `update_link_handler` — drop `extract_domain_from_headers`; read `domain_id`
  from the body (optional → default); resolve via `get_domain_by_id`.
- `delete_link_handler` — drop `Host`; read `domain_id` from a query struct
  (optional → default); resolve via `get_domain_by_id`.

`api/handlers/stats.rs` — `domain` (name) query param → `domain_id` (i64);
filter directly by id, omitted ⇒ no filter.

`api/dto/shorten.rs` — `UrlItem.domain: Option<String>` → `domain_id: Option<i64>`.
`api/dto/update_link.rs` — add `domain_id: Option<i64>`.
Stats query DTO — `domain: Option<String>` → `domain_id: Option<i64>`.

`api/routes.rs` — register:

```rust
.route("/links/batch-deactivate", post(batch_deactivate_handler))
.route("/links/batch-restore", post(batch_restore_handler))
```

## Data Flow

Batch deactivate:

1. Client `POST /links/batch-deactivate` `{codes, domain_id?}` (Bearer auth).
2. Handler validates DTO (1..=1000 codes), resolves domain (`domain_id` →
   `get_domain_by_id`, else `get_default_domain`) → 404 if id absent.
3. `LinkService::deactivate_links` de-dups codes, calls
   `LinkRepository::soft_delete_many` (one SQL `UPDATE ... RETURNING`), returns
   the affected codes.
4. Handler invalidates cache `{domain}:{code}` for each returned code
   (best-effort).
5. Handler builds `{summary, items}`: returned codes → `deactivated`; the rest
   of the unique input → `not_found`; `items` in original input order.

Restore is identical with `restore_many` and `restored` status.

## Error Handling

- Empty `codes` or `> 1000` → 400 (validator).
- Non-existent `domain_id` on mutating endpoints → 404 (via `get_domain_by_id`).
- Cache invalidation failures → logged `warn!`, never fail the request.
- DB errors → `AppError::Internal` (500), as elsewhere.
- Stats with a non-existent `domain_id` → empty result set (filter semantics),
  not an error — consistent with today's "unknown filter ⇒ no rows".

## Testing Strategy

**Unit (mockall, in-module):**

- `LinkService::deactivate_links` / `restore_links`: happy path, partial
  (some codes not returned → `not_found`), de-duplication (repeated code counted
  once), empty-after-dedup edge, cache-invalidation-error tolerated.
- Response assembly: ordering preserved; `summary` counts match `items`.

**Integration (`tests/`, axum-test `TestServer` + `serial_test`):**

- batch-deactivate happy path (all `deactivated`).
- batch-deactivate partial: mix of active, already-deleted, and unknown codes →
  correct `deactivated`/`not_found` split.
- Idempotency: second deactivate of the same codes → all `not_found`.
- batch-restore of previously deactivated codes → `restored`; restore of active
  codes → `not_found`.
- Validation: empty `codes` → 400; 1001 codes → 400.
- Default-domain behavior when `domain_id` omitted.
- Non-existent `domain_id` → 404.
- Updated `/shorten`, `/stats`, `PATCH`/`DELETE /links/{code}` tests under the
  `domain_id` selector (replacing `domain`-name / `Host` expectations).

**sqlx offline cache:** run `cargo sqlx prepare -- --all-targets` after adding
the new `query!`/`query_as!` calls so `.sqlx/` stays in sync (covers the
integration queries under `tests/` too).

## Migration / Compatibility Notes

- No database migration: deactivation reuses the existing `deleted_at` column.
- This is a **breaking API change** for clients using domain-name selectors or
  `Host`-based resolution on admin endpoints. Accepted: the product's only
  client is its authors. No deprecation window.
- Public redirect behavior (`GET /{code}` via `Host`) is unchanged.

## Documentation Updates

- `README.md`: document `POST /links/batch-deactivate` and
  `POST /links/batch-restore` (request/response, partial-success semantics,
  1000-code limit), and the unified `domain_id` selector across the management
  API. Add a "breaking change" note for the selector switch.
- `CLAUDE.md`: note the `domain_id` selector convention and the two new
  endpoints under the API/architecture sections.
