use crate::error::AppError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct ApiToken {
    pub id: i64,
    pub name: String,
    pub token_hash: String,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

/// Репозиторий для работы с API токенами
#[async_trait]
pub trait TokenRepository: Send + Sync {
    /// Проверить, валиден ли токен (существует и не отозван)
    async fn validate_token(&self, token_hash: &str) -> Result<bool, AppError>;

    /// Обновить время последнего использования токена
    async fn update_last_used(&self, token_hash: &str) -> Result<(), AppError>;

    /// Create new token
    async fn create_token(&self, name: &str, token_hash: &str) -> Result<ApiToken, AppError>;

    /// List all tokens
    async fn list_tokens(&self) -> Result<Vec<ApiToken>, AppError>;

    /// Find token by ID
    async fn find_by_id(&self, id: i64) -> Result<Option<ApiToken>, AppError>;

    /// Find token by name
    async fn find_by_name(&self, name: &str) -> Result<Option<ApiToken>, AppError>;

    /// Revoke token
    async fn revoke_token(&self, id: i64) -> Result<(), AppError>;
}
