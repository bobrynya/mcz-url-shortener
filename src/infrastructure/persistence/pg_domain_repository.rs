use async_trait::async_trait;
use sqlx::PgPool;
use std::sync::Arc;

use crate::domain::entities::{Domain, NewDomain, UpdateDomain};
use crate::domain::repositories::DomainRepository;
use crate::error::AppError;
use serde_json::json;

pub struct PgDomainRepository {
    pool: Arc<PgPool>,
}

impl PgDomainRepository {
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl DomainRepository for PgDomainRepository {
    async fn create(&self, new_domain: NewDomain) -> Result<Domain, AppError> {
        let row = sqlx::query!(
            r#"
            INSERT INTO domains (domain, is_default, description)
            VALUES ($1, $2, $3)
            RETURNING id, domain, is_default, is_active, description, created_at, updated_at
            "#,
            new_domain.domain,
            new_domain.is_default,
            new_domain.description
        )
        .fetch_one(self.pool.as_ref())
        .await?;

        Ok(Domain::new(
            row.id,
            row.domain,
            row.is_default,
            row.is_active,
            row.description,
            row.created_at,
            row.updated_at,
        ))
    }

    async fn find_by_id(&self, id: i64) -> Result<Option<Domain>, AppError> {
        let row = sqlx::query!(
            r#"
            SELECT id, domain, is_default, is_active, description, created_at, updated_at
            FROM domains
            WHERE id = $1
            "#,
            id
        )
        .fetch_optional(self.pool.as_ref())
        .await?;

        Ok(row.map(|r| {
            Domain::new(
                r.id,
                r.domain,
                r.is_default,
                r.is_active,
                r.description,
                r.created_at,
                r.updated_at,
            )
        }))
    }

    async fn find_by_name(&self, domain: &str) -> Result<Option<Domain>, AppError> {
        let row = sqlx::query!(
            r#"
            SELECT id, domain, is_default, is_active, description, created_at, updated_at
            FROM domains
            WHERE domain = $1
            "#,
            domain
        )
        .fetch_optional(self.pool.as_ref())
        .await?;

        Ok(row.map(|r| {
            Domain::new(
                r.id,
                r.domain,
                r.is_default,
                r.is_active,
                r.description,
                r.created_at,
                r.updated_at,
            )
        }))
    }

    async fn get_default(&self) -> Result<Domain, AppError> {
        let row = sqlx::query!(
            r#"
            SELECT id, domain, is_default, is_active, description, created_at, updated_at
            FROM domains
            WHERE is_default = TRUE
            LIMIT 1
            "#
        )
        .fetch_optional(self.pool.as_ref())
        .await?;

        row.map(|r| {
            Domain::new(
                r.id,
                r.domain,
                r.is_default,
                r.is_active,
                r.description,
                r.created_at,
                r.updated_at,
            )
        })
        .ok_or_else(|| {
            AppError::internal(
                "No default domain configured",
                json!({"hint": "Run migrations or create a default domain"}),
            )
        })
    }

    async fn list(&self, only_active: bool) -> Result<Vec<Domain>, AppError> {
        let rows = sqlx::query!(
            r#"
        SELECT id, domain, is_default, is_active, description, created_at, updated_at
        FROM domains
        WHERE ($1::boolean IS NULL OR is_active = $1)
        ORDER BY is_default DESC, domain
        "#,
            if only_active { Some(true) } else { None }
        )
        .fetch_all(self.pool.as_ref())
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                Domain::new(
                    r.id,
                    r.domain,
                    r.is_default,
                    r.is_active,
                    r.description,
                    r.created_at,
                    r.updated_at,
                )
            })
            .collect())
    }

    async fn update(&self, id: i64, update: UpdateDomain) -> Result<Domain, AppError> {
        let row = sqlx::query!(
            r#"
            UPDATE domains
            SET
                is_active = COALESCE($2, is_active),
                description = COALESCE($3, description),
                updated_at = NOW()
            WHERE id = $1
            RETURNING id, domain, is_default, is_active, description, created_at, updated_at
            "#,
            id,
            update.is_active,
            update.description
        )
        .fetch_one(self.pool.as_ref())
        .await?;

        Ok(Domain::new(
            row.id,
            row.domain,
            row.is_default,
            row.is_active,
            row.description,
            row.created_at,
            row.updated_at,
        ))
    }

    async fn delete(&self, id: i64) -> Result<(), AppError> {
        let result = sqlx::query!("DELETE FROM domains WHERE id = $1", id)
            .execute(self.pool.as_ref())
            .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::not_found("Domain not found", json!({"id": id})));
        }

        Ok(())
    }

    async fn set_default(&self, id: i64) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await?;

        // Снять флаг default со всех доменов
        sqlx::query!("UPDATE domains SET is_default = FALSE WHERE id >= 0")
            .execute(&mut *tx)
            .await?;

        // Установить флаг на нужный домен
        let result = sqlx::query!("UPDATE domains SET is_default = TRUE WHERE id = $1", id)
            .execute(&mut *tx)
            .await?;

        if result.rows_affected() == 0 {
            tx.rollback().await?;
            return Err(AppError::not_found("Domain not found", json!({"id": id})));
        }

        tx.commit().await?;
        Ok(())
    }

    async fn count_links(&self, domain_id: i64) -> Result<i64, AppError> {
        let count =
            sqlx::query_scalar!("SELECT COUNT(*) FROM links WHERE domain_id = $1", domain_id)
                .fetch_one(self.pool.as_ref())
                .await?;

        Ok(count.unwrap_or(0))
    }
}
