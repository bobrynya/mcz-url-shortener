-- Fix long_url uniqueness for the multi-domain + soft-delete model.
--
-- The original `links.long_url TEXT NOT NULL UNIQUE` constraint (from the very
-- first migration) enforced GLOBAL uniqueness of the destination URL. That is
-- incompatible with two later features:
--   1. Multi-domain: the same destination should be shortenable on different
--      domains independently.
--   2. Soft-delete: after a link is soft-deleted, re-creating a link for the
--      same URL must succeed, but the soft-deleted row keeps its `long_url`.
--
-- The application-level deduplication already scopes lookups per-domain and
-- excludes soft-deleted rows (see `find_by_long_url`). This migration aligns the
-- database constraint with that logic: uniqueness is now per (long_url, domain)
-- and only applies to live (non-deleted) rows.

ALTER TABLE links DROP CONSTRAINT IF EXISTS links_long_url_key;

CREATE UNIQUE INDEX IF NOT EXISTS idx_links_unique_long_url_per_domain
    ON links (long_url, COALESCE(domain_id, 0))
    WHERE deleted_at IS NULL;

-- The code uniqueness index suffered from the same flaw: it covered
-- soft-deleted rows, so a code could never be reused after its link was
-- deleted, even though the service layer explicitly treats a soft-deleted
-- code as free (`!l.is_deleted()` check before insert). Rebuild it as a
-- partial index scoped to live rows so the soft-delete/recreate flow works.
DROP INDEX IF EXISTS idx_links_unique_code_per_domain;

CREATE UNIQUE INDEX IF NOT EXISTS idx_links_unique_code_per_domain
    ON links (code, COALESCE(domain_id, 0))
    WHERE deleted_at IS NULL;
