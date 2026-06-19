//! PostgreSQL implementation of link repository.

use async_trait::async_trait;
use sqlx::PgPool;
use std::sync::Arc;

use crate::domain::entities::{Link, LinkPatch, NewLink};
use crate::domain::repositories::LinkRepository;
use crate::error::AppError;
use serde_json::json;

/// PostgreSQL repository for link storage and retrieval.
///
/// Uses SQLx prepared statements for SQL injection protection and type safety.
pub struct PgLinkRepository {
    pool: Arc<PgPool>,
}

impl PgLinkRepository {
    /// Creates a new repository with a database connection pool.
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl LinkRepository for PgLinkRepository {
    async fn create(&self, new_link: NewLink) -> Result<Link, AppError> {
        let row = sqlx::query!(
            r#"
            WITH inserted AS (
                INSERT INTO links (code, long_url, domain_id, expires_at, permanent)
                VALUES ($1, $2, $3, $4, $5)
                RETURNING id, code, long_url, domain_id, expires_at, permanent, deleted_at, created_at
            )
            SELECT
                i.id,
                i.code,
                i.long_url,
                d.domain,
                i.expires_at,
                i.permanent,
                i.deleted_at,
                i.created_at
            FROM inserted i
            LEFT JOIN domains d ON d.id = i.domain_id
            "#,
            new_link.code,
            new_link.long_url,
            new_link.domain_id,
            new_link.expires_at,
            new_link.permanent,
        )
        .fetch_one(self.pool.as_ref())
        .await?;

        Ok(Link::new(
            row.id,
            row.code,
            row.long_url,
            row.domain,
            row.created_at,
            row.expires_at,
            row.permanent,
            row.deleted_at,
        ))
    }

    async fn find_by_code(&self, code: &str, domain_id: i64) -> Result<Option<Link>, AppError> {
        // Does NOT filter deleted_at — caller decides what to do with deleted links.
        let row = sqlx::query!(
            r#"
            SELECT
                l.id, l.code, l.long_url,
                d.domain as "domain?",
                l.expires_at, l.permanent, l.deleted_at, l.created_at
            FROM links l
            LEFT JOIN domains d ON d.id = l.domain_id
            WHERE l.code = $1 AND l.domain_id = $2
            "#,
            code,
            domain_id
        )
        .fetch_optional(self.pool.as_ref())
        .await?;

        Ok(row.map(|r| {
            Link::new(
                r.id,
                r.code,
                r.long_url,
                r.domain,
                r.created_at,
                r.expires_at,
                r.permanent,
                r.deleted_at,
            )
        }))
    }

    async fn find_any_by_code(&self, code: &str) -> Result<Option<Link>, AppError> {
        // Resolves a code across all domains; returns the most recently created match.
        // Does NOT filter deleted_at — caller decides what to do with deleted links.
        let row = sqlx::query!(
            r#"
            SELECT
                l.id, l.code, l.long_url,
                d.domain as "domain?",
                l.expires_at, l.permanent, l.deleted_at, l.created_at
            FROM links l
            LEFT JOIN domains d ON d.id = l.domain_id
            WHERE l.code = $1
            ORDER BY l.created_at DESC
            LIMIT 1
            "#,
            code
        )
        .fetch_optional(self.pool.as_ref())
        .await?;

        Ok(row.map(|r| {
            Link::new(
                r.id,
                r.code,
                r.long_url,
                r.domain,
                r.created_at,
                r.expires_at,
                r.permanent,
                r.deleted_at,
            )
        }))
    }

    async fn find_by_long_url(
        &self,
        long_url: &str,
        domain_id: i64,
    ) -> Result<Option<Link>, AppError> {
        // Filters out deleted links so a new link can be created for the same URL after delete.
        let row = sqlx::query!(
            r#"
            SELECT
                l.id, l.code, l.long_url,
                d.domain as "domain?",
                l.expires_at, l.permanent, l.deleted_at, l.created_at
            FROM links l
            LEFT JOIN domains d ON d.id = l.domain_id
            WHERE l.long_url = $1 AND l.domain_id = $2 AND l.deleted_at IS NULL
            "#,
            long_url,
            domain_id
        )
        .fetch_optional(self.pool.as_ref())
        .await?;

        Ok(row.map(|r| {
            Link::new(
                r.id,
                r.code,
                r.long_url,
                r.domain,
                r.created_at,
                r.expires_at,
                r.permanent,
                r.deleted_at,
            )
        }))
    }

    async fn list(
        &self,
        page: i64,
        page_size: i64,
        domain_id: Option<i64>,
    ) -> Result<Vec<Link>, AppError> {
        let offset = (page - 1) * page_size;

        // Returns all links including soft-deleted for stats/dashboard visibility.
        let rows = sqlx::query!(
            r#"
            SELECT
                l.id, l.code, l.long_url,
                d.domain as "domain?",
                l.expires_at, l.permanent, l.deleted_at, l.created_at
            FROM links l
            LEFT JOIN domains d ON d.id = l.domain_id
            WHERE ($1::bigint IS NULL OR l.domain_id = $1)
            ORDER BY l.created_at DESC
            LIMIT $2 OFFSET $3
            "#,
            domain_id,
            page_size,
            offset
        )
        .fetch_all(self.pool.as_ref())
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                Link::new(
                    r.id,
                    r.code,
                    r.long_url,
                    r.domain,
                    r.created_at,
                    r.expires_at,
                    r.permanent,
                    r.deleted_at,
                )
            })
            .collect())
    }

    async fn count(&self, domain_id: Option<i64>) -> Result<i64, AppError> {
        let count = if let Some(domain_id) = domain_id {
            sqlx::query_scalar!("SELECT COUNT(*) FROM links WHERE domain_id = $1", domain_id)
                .fetch_one(self.pool.as_ref())
                .await?
        } else {
            sqlx::query_scalar!("SELECT COUNT(*) FROM links")
                .fetch_one(self.pool.as_ref())
                .await?
        };

        Ok(count.unwrap_or(0))
    }

    async fn soft_delete(&self, code: &str, domain_id: i64) -> Result<bool, AppError> {
        let result = sqlx::query!(
            r#"
            UPDATE links
            SET deleted_at = now()
            WHERE code = $1 AND domain_id = $2 AND deleted_at IS NULL
            "#,
            code,
            domain_id
        )
        .execute(self.pool.as_ref())
        .await?;

        Ok(result.rows_affected() > 0)
    }

    async fn update(&self, code: &str, domain_id: i64, patch: LinkPatch) -> Result<Link, AppError> {
        let update_expires = patch.expires_at.is_some();
        let new_expires = patch.expires_at.and_then(|v| v);

        let row = sqlx::query!(
            r#"
            WITH updated AS (
                UPDATE links SET
                    long_url   = COALESCE($3::TEXT,    long_url),
                    expires_at = CASE WHEN $4 THEN $5::TIMESTAMPTZ ELSE expires_at END,
                    permanent  = COALESCE($6::BOOLEAN, permanent),
                    deleted_at = CASE WHEN $7 THEN NULL ELSE deleted_at END
                WHERE code = $1 AND domain_id = $2
                RETURNING id, code, long_url, domain_id, expires_at, permanent, deleted_at, created_at
            )
            SELECT
                u.id, u.code, u.long_url,
                d.domain,
                u.expires_at, u.permanent, u.deleted_at, u.created_at
            FROM updated u
            LEFT JOIN domains d ON d.id = u.domain_id
            "#,
            code,
            domain_id,
            patch.url,
            update_expires,
            new_expires,
            patch.permanent,
            patch.restore,
        )
        .fetch_optional(self.pool.as_ref())
        .await?
        .ok_or_else(|| AppError::not_found("Link not found", json!({ "code": code })))?;

        Ok(Link::new(
            row.id,
            row.code,
            row.long_url,
            row.domain,
            row.created_at,
            row.expires_at,
            row.permanent,
            row.deleted_at,
        ))
    }
}
