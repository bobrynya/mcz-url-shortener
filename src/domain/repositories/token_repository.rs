use crate::error::AppError;
use async_trait::async_trait;

/// Репозиторий для работы с API токенами
#[async_trait]
pub trait TokenRepository: Send + Sync {
    /// Проверить, валиден ли токен (существует и не отозван)
    async fn validate_token(&self, token_hash: &str) -> Result<bool, AppError>;

    /// Обновить время последнего использования токена
    async fn update_last_used(&self, token_hash: &str) -> Result<(), AppError>;
}
