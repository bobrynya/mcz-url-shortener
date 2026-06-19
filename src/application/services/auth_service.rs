//! Authentication service for API token validation.

use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::sync::Arc;

use crate::domain::repositories::TokenRepository;
use crate::error::AppError;
use serde_json::json;

type HmacSha256 = Hmac<Sha256>;

/// Service for authenticating API requests via Bearer tokens.
///
/// Tokens are hashed with HMAC-SHA256 (keyed by `signing_secret`) before storage
/// and comparison. An attacker with read-only access to the database cannot verify
/// or forge tokens without the server-side secret.
pub struct AuthService<R: TokenRepository> {
    repository: Arc<R>,
    signing_secret: String,
}

impl<R: TokenRepository> AuthService<R> {
    /// Creates a new authentication service.
    ///
    /// # Arguments
    ///
    /// - `repository` - token repository for DB operations
    /// - `signing_secret` - HMAC key; must match the value used when tokens were created
    pub fn new(repository: Arc<R>, signing_secret: String) -> Self {
        Self {
            repository,
            signing_secret,
        }
    }

    /// Hashes a raw token with HMAC-SHA256 using the server signing secret.
    ///
    /// Returns a 64-character lowercase hex-encoded MAC.
    fn hash_token(&self, token: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(self.signing_secret.as_bytes())
            .expect("HMAC accepts any key length");
        mac.update(token.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    /// Authenticates a raw token against stored credentials.
    ///
    /// On successful authentication, updates the `last_used` timestamp for
    /// monitoring and audit purposes.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Unauthorized`] if:
    /// - Token hash does not match any stored credentials
    /// - Token has been revoked
    ///
    /// Returns [`AppError::Internal`] on database errors.
    pub async fn authenticate(&self, token: &str) -> Result<(), AppError> {
        let token_hash = self.hash_token(token);

        let is_valid = self.repository.validate_token(&token_hash).await?;

        if !is_valid {
            return Err(AppError::unauthorized(
                "Unauthorized",
                json!({"reason": "Invalid or revoked token"}),
            ));
        }

        let _ = self.repository.update_last_used(&token_hash).await;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::repositories::MockTokenRepository;

    fn test_secret() -> String {
        "test-signing-secret".to_string()
    }

    fn compute_expected_hash(token: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(test_secret().as_bytes())
            .expect("HMAC accepts any key length");
        mac.update(token.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    #[tokio::test]
    async fn test_authenticate_success() {
        let mut mock_repo = MockTokenRepository::new();

        let token = "valid-token";
        let expected_hash = compute_expected_hash(token);

        mock_repo
            .expect_validate_token()
            .withf(move |hash| *hash == expected_hash)
            .times(1)
            .returning(|_| Ok(true));

        mock_repo
            .expect_update_last_used()
            .times(1)
            .returning(|_| Ok(()));

        let service = AuthService::new(Arc::new(mock_repo), test_secret());

        let result = service.authenticate(token).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_authenticate_invalid_token() {
        let mut mock_repo = MockTokenRepository::new();

        mock_repo
            .expect_validate_token()
            .times(1)
            .returning(|_| Ok(false));

        let service = AuthService::new(Arc::new(mock_repo), test_secret());

        let result = service.authenticate("invalid-token").await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::Unauthorized { .. }));
    }

    #[tokio::test]
    async fn test_hash_token_consistency() {
        let mock_repo = MockTokenRepository::new();
        let service = AuthService::new(Arc::new(mock_repo), test_secret());

        let hash1 = service.hash_token("test-token");
        let hash2 = service.hash_token("test-token");

        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 64);
    }

    #[tokio::test]
    async fn test_hash_token_different_inputs() {
        let mock_repo = MockTokenRepository::new();
        let service = AuthService::new(Arc::new(mock_repo), test_secret());

        let hash1 = service.hash_token("token1");
        let hash2 = service.hash_token("token2");

        assert_ne!(hash1, hash2);
    }

    #[tokio::test]
    async fn test_hash_token_secret_matters() {
        let mock_repo1 = MockTokenRepository::new();
        let mock_repo2 = MockTokenRepository::new();

        let svc1 = AuthService::new(Arc::new(mock_repo1), "secret-a".to_string());
        let svc2 = AuthService::new(Arc::new(mock_repo2), "secret-b".to_string());

        // Same token, different secrets → different hashes
        assert_ne!(svc1.hash_token("token"), svc2.hash_token("token"));
    }
}
