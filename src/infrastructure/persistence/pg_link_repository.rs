use async_trait::async_trait;
use sqlx::PgPool;
use std::sync::Arc;

use crate::domain::entities::{Link, NewLink};
use crate::domain::repositories::LinkRepository;
use crate::error::AppError;

pub struct PgLinkRepository {
    pool: Arc<PgPool>,
}

impl PgLinkRepository {
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
            INSERT INTO links (code, long_url, domain_id)
            VALUES ($1, $2, $3)
            RETURNING id, code, long_url, domain_id, created_at
        )
        SELECT
            i.id,
            i.code,
            i.long_url,
            d.domain,
            i.created_at
        FROM inserted i
        LEFT JOIN domains d ON d.id = i.domain_id
        "#,
            new_link.code,
            new_link.long_url,
            new_link.domain_id
        )
        .fetch_one(self.pool.as_ref())
        .await?;

        Ok(Link::new(
            row.id,
            row.code,
            row.long_url,
            row.domain,
            row.created_at,
        ))
    }

    async fn find_by_code(&self, code: &str, domain_id: i64) -> Result<Option<Link>, AppError> {
        let row = sqlx::query!(
            r#"
            SELECT l.id, l.code, l.long_url, d.domain as "domain?", l.created_at
            FROM links l
            LEFT JOIN domains d ON d.id = l.domain_id
            WHERE code = $1 AND domain_id = $2
            "#,
            code,
            domain_id
        )
        .fetch_optional(self.pool.as_ref())
        .await?;

        Ok(row.map(|r| Link::new(r.id, r.code, r.long_url, r.domain, r.created_at)))
    }

    async fn find_by_long_url(
        &self,
        long_url: &str,
        domain_id: i64,
    ) -> Result<Option<Link>, AppError> {
        let row = sqlx::query!(
            r#"
            SELECT l.id, l.code, l.long_url, d.domain as "domain?", l.created_at
            FROM links l
            LEFT JOIN domains d ON d.id = l.domain_id
            WHERE long_url = $1 AND domain_id = $2
            "#,
            long_url,
            domain_id
        )
        .fetch_optional(self.pool.as_ref())
        .await?;

        Ok(row.map(|r| Link::new(r.id, r.code, r.long_url, r.domain, r.created_at)))
    }

    async fn list(
        &self,
        page: i64,
        page_size: i64,
        domain_id: Option<i64>,
    ) -> Result<Vec<Link>, AppError> {
        let offset = (page - 1) * page_size;

        // ИСПРАВЛЕНО: один запрос с условием
        let rows = sqlx::query!(
            r#"
        SELECT l.id, l.code, l.long_url, d.domain as "domain?", l.created_at
        FROM links l
        LEFT JOIN domains d ON d.id = l.domain_id
        WHERE ($1::bigint IS NULL OR domain_id = $1)
        ORDER BY created_at DESC
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
            .map(|r| Link::new(r.id, r.code, r.long_url, r.domain, r.created_at))
            .collect())
    }

    // ДОБАВЛЕНО: реализация count
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
}
