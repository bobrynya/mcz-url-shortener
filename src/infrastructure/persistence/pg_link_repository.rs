use async_trait::async_trait;
use sqlx::PgPool;
use std::sync::Arc;

use crate::domain::entities::{Link, NewLink};
use crate::domain::repositories::LinkRepository;
use crate::error::AppError;

/// PostgreSQL реализация репозитория ссылок
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
            INSERT INTO links (code, long_url)
            VALUES ($1, $2)
            RETURNING id, code, long_url, created_at
            "#,
            new_link.code,
            new_link.long_url
        )
        .fetch_one(self.pool.as_ref())
        .await?;

        Ok(Link::new(row.id, row.code, row.long_url, row.created_at))
    }

    async fn find_by_code(&self, code: &str) -> Result<Option<Link>, AppError> {
        let row = sqlx::query!(
            r#"
            SELECT id, code, long_url, created_at
            FROM links
            WHERE code = $1
            "#,
            code
        )
        .fetch_optional(self.pool.as_ref())
        .await?;

        Ok(row.map(|r| Link::new(r.id, r.code, r.long_url, r.created_at)))
    }

    async fn find_by_long_url(&self, long_url: &str) -> Result<Option<Link>, AppError> {
        let row = sqlx::query!(
            r#"
            SELECT id, code, long_url, created_at
            FROM links
            WHERE long_url = $1
            "#,
            long_url
        )
        .fetch_optional(self.pool.as_ref())
        .await?;

        Ok(row.map(|r| Link::new(r.id, r.code, r.long_url, r.created_at)))
    }
}
