use async_trait::async_trait;
use sqlx::PgPool;
use std::sync::Arc;

use crate::domain::repositories::{ApiToken, TokenRepository};
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

    async fn create_token(&self, name: &str, token_hash: &str) -> Result<ApiToken, AppError> {
        let row = sqlx::query!(
            r#"
            INSERT INTO api_tokens (name, token_hash)
            VALUES ($1, $2)
            RETURNING id, name, token_hash, created_at, revoked_at
            "#,
            name,
            token_hash
        )
        .fetch_one(self.pool.as_ref())
        .await?;

        Ok(ApiToken {
            id: row.id,
            name: row.name,
            token_hash: row.token_hash,
            created_at: row.created_at,
            revoked_at: row.revoked_at,
        })
    }

    async fn list_tokens(&self) -> Result<Vec<ApiToken>, AppError> {
        let rows = sqlx::query!(
            r#"
            SELECT id, name, token_hash, created_at, revoked_at
            FROM api_tokens
            ORDER BY created_at DESC
            "#
        )
        .fetch_all(self.pool.as_ref())
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| ApiToken {
                id: row.id,
                name: row.name,
                token_hash: row.token_hash,
                created_at: row.created_at,
                revoked_at: row.revoked_at,
            })
            .collect())
    }

    async fn find_by_id(&self, id: i64) -> Result<Option<ApiToken>, AppError> {
        let row = sqlx::query!(
            r#"
            SELECT id, name, token_hash, created_at, revoked_at
            FROM api_tokens
            WHERE id = $1
            "#,
            id
        )
        .fetch_optional(self.pool.as_ref())
        .await?;

        Ok(row.map(|r| ApiToken {
            id: r.id,
            name: r.name,
            token_hash: r.token_hash,
            created_at: r.created_at,
            revoked_at: r.revoked_at,
        }))
    }

    async fn find_by_name(&self, name: &str) -> Result<Option<ApiToken>, AppError> {
        let row = sqlx::query!(
            r#"
            SELECT id, name, token_hash, created_at, revoked_at
            FROM api_tokens
            WHERE name = $1
            "#,
            name
        )
        .fetch_optional(self.pool.as_ref())
        .await?;

        Ok(row.map(|r| ApiToken {
            id: r.id,
            name: r.name,
            token_hash: r.token_hash,
            created_at: r.created_at,
            revoked_at: r.revoked_at,
        }))
    }

    async fn revoke_token(&self, id: i64) -> Result<(), AppError> {
        sqlx::query!(
            r#"
            UPDATE api_tokens
            SET revoked_at = NOW()
            WHERE id = $1 AND revoked_at IS NULL
            "#,
            id
        )
        .execute(self.pool.as_ref())
        .await?;

        Ok(())
    }
}
