use async_trait::async_trait;
use sqlx::PgPool;
use std::sync::Arc;

use crate::domain::repositories::TokenRepository;
use crate::error::AppError;

/// PostgreSQL реализация репозитория токенов
pub struct PgTokenRepository {
    pool: Arc<PgPool>,
}

impl PgTokenRepository {
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TokenRepository for PgTokenRepository {
    async fn validate_token(&self, token_hash: &str) -> Result<bool, AppError> {
        let row = sqlx::query!(
            r#"
            SELECT id
            FROM api_tokens
            WHERE token_hash = $1
              AND revoked_at IS NULL
            "#,
            token_hash
        )
        .fetch_optional(self.pool.as_ref())
        .await?;

        Ok(row.is_some())
    }

    async fn update_last_used(&self, token_hash: &str) -> Result<(), AppError> {
        sqlx::query!(
            r#"
            UPDATE api_tokens
            SET last_used_at = NOW()
            WHERE token_hash = $1
              AND revoked_at IS NULL
            "#,
            token_hash
        )
        .execute(self.pool.as_ref())
        .await?;

        Ok(())
    }
}
