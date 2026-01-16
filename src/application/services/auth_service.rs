use sha2::{Digest, Sha256};
use std::sync::Arc;

use crate::domain::repositories::TokenRepository;
use crate::error::AppError;
use serde_json::json;

/// Сервис для аутентификации
pub struct AuthService<R: TokenRepository> {
    repository: Arc<R>,
}

impl<R: TokenRepository> AuthService<R> {
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }

    fn hash_token(&self, token: &str) -> String {
        let mut h = Sha256::new();
        h.update(token.as_bytes());
        hex::encode(h.finalize())
    }

    pub async fn authenticate(&self, token: &str) -> Result<(), AppError> {
        let token_hash = self.hash_token(token);

        // Проверяем валидность токена
        let is_valid = self.repository.validate_token(&token_hash).await?;

        if !is_valid {
            return Err(AppError::unauthorized(
                "Unauthorized",
                json!({"reason": "Invalid or revoked token"}),
            ));
        }

        // Обновляем время последнего использования
        let _ = self.repository.update_last_used(&token_hash).await;

        Ok(())
    }
}
