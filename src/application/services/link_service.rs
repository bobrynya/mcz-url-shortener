//! Link creation, retrieval, deletion, and update service.

use std::sync::Arc;

use crate::domain::entities::{Link, LinkPatch, NewLink};
use crate::domain::repositories::{DomainRepository, LinkRepository};
use crate::error::AppError;
use crate::utils::code_generator::{generate_code, validate_custom_code};
use crate::utils::url_normalizer::{is_public_url, normalize_url};
use chrono::{DateTime, Utc};
use serde_json::json;

/// Service for creating and managing shortened links.
///
/// Handles URL normalization, code generation/validation, deduplication,
/// soft-deletion, and partial updates.
pub struct LinkService<L: LinkRepository, D: DomainRepository> {
    link_repository: Arc<L>,
    domain_repository: Arc<D>,
    /// When true, reject URLs that resolve to private/loopback/local hosts.
    block_private_urls: bool,
}

impl<L: LinkRepository, D: DomainRepository> LinkService<L, D> {
    /// Creates a new link service.
    ///
    /// `block_private_urls` controls whether destinations pointing at
    /// private/loopback/link-local hosts (or `localhost`) are rejected.
    pub fn new(
        link_repository: Arc<L>,
        domain_repository: Arc<D>,
        block_private_urls: bool,
    ) -> Self {
        Self {
            link_repository,
            domain_repository,
            block_private_urls,
        }
    }

    /// Creates a short link using the default domain.
    pub async fn create_short_link(
        &self,
        long_url: String,
        custom_code: Option<String>,
        expires_at: Option<DateTime<Utc>>,
        permanent: bool,
    ) -> Result<Link, AppError> {
        let default_domain = self.domain_repository.get_default().await?;
        self.create_short_link_for_domain(
            long_url,
            default_domain.id,
            custom_code,
            expires_at,
            permanent,
        )
        .await
    }

    /// Creates a short link for a specific domain.
    ///
    /// # Deduplication
    ///
    /// If a non-deleted link for the same normalized URL and domain already exists,
    /// returns the existing link instead of creating a duplicate.
    ///
    /// # Code Generation
    ///
    /// - If `custom_code` is provided, validates and uses it (or returns conflict error)
    /// - Otherwise, generates a cryptographically secure random 12-character code
    /// - Retries up to 10 times on collision before failing
    pub async fn create_short_link_for_domain(
        &self,
        long_url: String,
        domain_id: i64,
        custom_code: Option<String>,
        expires_at: Option<DateTime<Utc>>,
        permanent: bool,
    ) -> Result<Link, AppError> {
        let normalized_url = normalize_url(&long_url).map_err(|e| {
            AppError::bad_request("Invalid URL format", json!({ "reason": e.to_string() }))
        })?;

        if self.block_private_urls && !is_public_url(&normalized_url) {
            return Err(AppError::bad_request(
                "URL points to a private, loopback, or local address",
                json!({ "reason": "private_or_local_host" }),
            ));
        }

        if let Some(existing_link) = self
            .link_repository
            .find_by_long_url(&normalized_url, domain_id)
            .await?
        {
            return Ok(existing_link);
        }

        let code = if let Some(custom) = custom_code {
            validate_custom_code(&custom)?;

            if self
                .link_repository
                .find_by_code(&custom, domain_id)
                .await?
                .is_some_and(|l| !l.is_deleted())
            {
                return Err(AppError::conflict(
                    "Custom code already exists for this domain",
                    json!({ "code": custom, "domain_id": domain_id }),
                ));
            }

            custom
        } else {
            self.generate_unique_code(domain_id).await?
        };

        let new_link = NewLink {
            code,
            long_url: normalized_url,
            domain_id,
            expires_at,
            permanent,
        };

        let created = self.link_repository.create(new_link).await?;
        metrics::counter!("links_created_total").increment(1);
        Ok(created)
    }

    /// Retrieves a link by its short code and domain.
    ///
    /// Returns the link regardless of deleted/expired state — callers check those fields.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::NotFound`] if no link matches the code and domain.
    pub async fn get_link_by_code(&self, code: &str, domain_id: i64) -> Result<Link, AppError> {
        self.link_repository
            .find_by_code(code, domain_id)
            .await?
            .ok_or_else(|| {
                AppError::not_found(
                    "Short link not found",
                    json!({ "code": code, "domain_id": domain_id }),
                )
            })
    }

    /// Constructs the full short URL from a domain and code.
    ///
    /// Always uses HTTPS protocol.
    pub fn get_short_url(&self, domain: &str, code: &str) -> String {
        format!("https://{}/{}", domain.trim_end_matches('/'), code)
    }

    /// Soft-deletes a link (sets `deleted_at`). Returns `false` if not found.
    pub async fn soft_delete_link(&self, code: &str, domain_id: i64) -> Result<bool, AppError> {
        self.link_repository.soft_delete(code, domain_id).await
    }

    /// Partially updates a link.
    ///
    /// Only patch fields that are `Some` are modified. Set `patch.restore = true`
    /// to restore a previously soft-deleted link.
    pub async fn update_link(
        &self,
        code: &str,
        domain_id: i64,
        patch: LinkPatch,
    ) -> Result<Link, AppError> {
        self.link_repository.update(code, domain_id, patch).await
    }

    /// Bulk-deactivates `codes` within `domain_id`. Input is de-duplicated
    /// (first occurrence kept). Returns the codes actually transitioned
    /// (were active, now soft-deleted).
    pub async fn deactivate_links(
        &self,
        codes: Vec<String>,
        domain_id: i64,
    ) -> Result<Vec<String>, AppError> {
        let unique = dedup_preserving_order(codes);
        self.link_repository
            .soft_delete_many(&unique, domain_id)
            .await
    }

    /// Bulk-restores `codes` within `domain_id`. Input is de-duplicated
    /// (first occurrence kept). Returns the codes actually transitioned
    /// (were soft-deleted, now active).
    pub async fn restore_links(
        &self,
        codes: Vec<String>,
        domain_id: i64,
    ) -> Result<Vec<String>, AppError> {
        let unique = dedup_preserving_order(codes);
        self.link_repository.restore_many(&unique, domain_id).await
    }

    /// Generates a unique short code for a domain with collision retry.
    async fn generate_unique_code(&self, domain_id: i64) -> Result<String, AppError> {
        const MAX_ATTEMPTS: usize = 10;

        for _ in 0..MAX_ATTEMPTS {
            let code = generate_code();

            if self
                .link_repository
                .find_by_code(&code, domain_id)
                .await?
                .is_none()
            {
                return Ok(code);
            }
        }

        Err(AppError::internal(
            "Failed to generate unique code",
            json!({ "reason": "Too many collisions" }),
        ))
    }
}

/// De-duplicates codes, preserving the order of each code's first occurrence.
fn dedup_preserving_order(codes: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    codes
        .into_iter()
        .filter(|c| seen.insert(c.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::Domain;
    use crate::domain::repositories::{MockDomainRepository, MockLinkRepository};
    use chrono::Utc;

    fn create_test_domain(id: i64) -> Domain {
        Domain::new(
            id,
            "s.example.com".to_string(),
            true,
            true,
            None,
            Utc::now(),
            Utc::now(),
            None,
        )
    }

    fn create_test_link(id: i64, code: &str, url: &str, _domain_id: i64) -> Link {
        Link::new(
            id,
            code.to_string(),
            url.to_string(),
            Some("s.example.com".to_string()),
            Utc::now(),
            None,
            false,
            None,
        )
    }

    #[tokio::test]
    async fn test_create_short_link_success() {
        let mut mock_link_repo = MockLinkRepository::new();
        let mut mock_domain_repo = MockDomainRepository::new();

        let domain = create_test_domain(1);
        mock_domain_repo
            .expect_get_default()
            .times(1)
            .returning(move || Ok(domain.clone()));

        mock_link_repo
            .expect_find_by_long_url()
            .times(1)
            .returning(|_, _| Ok(None));

        mock_link_repo
            .expect_find_by_code()
            .times(1)
            .returning(|_, _| Ok(None));

        let created_link = create_test_link(10, "abc123", "https://example.com", 1);
        mock_link_repo
            .expect_create()
            .times(1)
            .returning(move |_| Ok(created_link.clone()));

        let service = LinkService::new(Arc::new(mock_link_repo), Arc::new(mock_domain_repo), true);

        let result = service
            .create_short_link("https://example.com".to_string(), None, None, false)
            .await;

        assert!(result.is_ok());
        let link = result.unwrap();
        assert_eq!(link.long_url, "https://example.com");
    }

    #[tokio::test]
    async fn test_create_short_link_normalizes_url() {
        let mut mock_link_repo = MockLinkRepository::new();
        let mut mock_domain_repo = MockDomainRepository::new();

        let domain = create_test_domain(1);
        mock_domain_repo
            .expect_get_default()
            .times(1)
            .returning(move || Ok(domain.clone()));

        mock_link_repo
            .expect_find_by_long_url()
            .withf(|url, _| url == "https://example.com/path")
            .times(1)
            .returning(|_, _| Ok(None));

        mock_link_repo
            .expect_find_by_code()
            .times(1)
            .returning(|_, _| Ok(None));

        let created_link = create_test_link(10, "abc123", "https://example.com/path", 1);
        mock_link_repo
            .expect_create()
            .times(1)
            .returning(move |_| Ok(created_link.clone()));

        let service = LinkService::new(Arc::new(mock_link_repo), Arc::new(mock_domain_repo), true);

        let result = service
            .create_short_link(
                "https://EXAMPLE.COM:443/path".to_string(),
                None,
                None,
                false,
            )
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_create_short_link_deduplication() {
        let mut mock_link_repo = MockLinkRepository::new();
        let mut mock_domain_repo = MockDomainRepository::new();

        let domain = create_test_domain(1);
        mock_domain_repo
            .expect_get_default()
            .times(1)
            .returning(move || Ok(domain.clone()));

        let existing_link = create_test_link(5, "existing", "https://example.com", 1);
        mock_link_repo
            .expect_find_by_long_url()
            .times(1)
            .returning(move |_, _| Ok(Some(existing_link.clone())));

        mock_link_repo.expect_create().times(0);

        let service = LinkService::new(Arc::new(mock_link_repo), Arc::new(mock_domain_repo), true);

        let result = service
            .create_short_link("https://example.com".to_string(), None, None, false)
            .await;

        assert!(result.is_ok());
        let link = result.unwrap();
        assert_eq!(link.id, 5);
        assert_eq!(link.code, "existing");
    }

    #[tokio::test]
    async fn test_create_short_link_invalid_url() {
        let mock_link_repo = MockLinkRepository::new();
        let mock_domain_repo = MockDomainRepository::new();

        let service = LinkService::new(Arc::new(mock_link_repo), Arc::new(mock_domain_repo), true);

        let result = service
            .create_short_link_for_domain("not-a-url".to_string(), 1, None, None, false)
            .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::Validation { .. }));
    }

    #[tokio::test]
    async fn test_create_short_link_with_custom_code() {
        let mut mock_link_repo = MockLinkRepository::new();
        let mut mock_domain_repo = MockDomainRepository::new();

        let domain = create_test_domain(1);
        mock_domain_repo
            .expect_get_default()
            .times(1)
            .returning(move || Ok(domain.clone()));

        mock_link_repo
            .expect_find_by_long_url()
            .times(1)
            .returning(|_, _| Ok(None));

        mock_link_repo
            .expect_find_by_code()
            .withf(|code, _| code == "mycode12")
            .times(1)
            .returning(|_, _| Ok(None));

        let created_link = create_test_link(10, "mycode12", "https://example.com", 1);
        mock_link_repo
            .expect_create()
            .withf(|new_link| new_link.code == "mycode12")
            .times(1)
            .returning(move |_| Ok(created_link.clone()));

        let service = LinkService::new(Arc::new(mock_link_repo), Arc::new(mock_domain_repo), true);

        let result = service
            .create_short_link(
                "https://example.com".to_string(),
                Some("mycode12".to_string()),
                None,
                false,
            )
            .await;

        assert!(result.is_ok());
        let link = result.unwrap();
        assert_eq!(link.code, "mycode12");
    }

    #[tokio::test]
    async fn test_create_short_link_custom_code_conflict() {
        let mut mock_link_repo = MockLinkRepository::new();
        let mut mock_domain_repo = MockDomainRepository::new();

        let domain = create_test_domain(1);
        mock_domain_repo
            .expect_get_default()
            .times(1)
            .returning(move || Ok(domain.clone()));

        mock_link_repo
            .expect_find_by_long_url()
            .times(1)
            .returning(|_, _| Ok(None));

        let existing_link = create_test_link(5, "taken123", "https://other.com", 1);
        mock_link_repo
            .expect_find_by_code()
            .withf(|code, _| code == "taken123")
            .times(1)
            .returning(move |_, _| Ok(Some(existing_link.clone())));

        let service = LinkService::new(Arc::new(mock_link_repo), Arc::new(mock_domain_repo), true);

        let result = service
            .create_short_link(
                "https://example.com".to_string(),
                Some("taken123".to_string()),
                None,
                false,
            )
            .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::Conflict { .. }));
    }

    #[tokio::test]
    async fn test_create_short_link_blocks_private_url() {
        // No repository calls are expected — the check short-circuits before them.
        let mock_link_repo = MockLinkRepository::new();
        let mock_domain_repo = MockDomainRepository::new();

        let service = LinkService::new(Arc::new(mock_link_repo), Arc::new(mock_domain_repo), true);

        let result = service
            .create_short_link_for_domain(
                "http://localhost:8080/admin".to_string(),
                1,
                None,
                None,
                false,
            )
            .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::Validation { .. }));
    }

    #[tokio::test]
    async fn test_create_short_link_allows_private_url_when_disabled() {
        let mut mock_link_repo = MockLinkRepository::new();
        let mock_domain_repo = MockDomainRepository::new();

        mock_link_repo
            .expect_find_by_long_url()
            .times(1)
            .returning(|_, _| Ok(None));
        mock_link_repo
            .expect_find_by_code()
            .times(1)
            .returning(|_, _| Ok(None));
        let created = create_test_link(7, "abcd1234", "http://127.0.0.1:9000/", 1);
        mock_link_repo
            .expect_create()
            .times(1)
            .returning(move |_| Ok(created.clone()));

        // block_private_urls = false → loopback destinations are accepted.
        let service = LinkService::new(Arc::new(mock_link_repo), Arc::new(mock_domain_repo), false);

        let result = service
            .create_short_link_for_domain("http://127.0.0.1:9000".to_string(), 1, None, None, false)
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_deactivate_links_dedups_and_returns_affected() {
        let mut mock_link_repo = MockLinkRepository::new();
        let mock_domain_repo = MockDomainRepository::new();

        // Dedup keeps first occurrence; "a","b" only (the repeated "a" collapses).
        mock_link_repo
            .expect_soft_delete_many()
            .withf(|codes, domain_id| {
                *domain_id == 1 && codes == ["a".to_string(), "b".to_string()]
            })
            .times(1)
            .returning(|_, _| Ok(vec!["a".to_string()]));

        let service = LinkService::new(Arc::new(mock_link_repo), Arc::new(mock_domain_repo), true);

        let affected = service
            .deactivate_links(vec!["a".to_string(), "b".to_string(), "a".to_string()], 1)
            .await
            .unwrap();

        assert_eq!(affected, vec!["a".to_string()]);
    }

    #[tokio::test]
    async fn test_restore_links_delegates_to_repo() {
        let mut mock_link_repo = MockLinkRepository::new();
        let mock_domain_repo = MockDomainRepository::new();

        mock_link_repo
            .expect_restore_many()
            .withf(|codes, domain_id| *domain_id == 2 && codes == ["x".to_string()])
            .times(1)
            .returning(|_, _| Ok(vec!["x".to_string()]));

        let service = LinkService::new(Arc::new(mock_link_repo), Arc::new(mock_domain_repo), true);

        let affected = service
            .restore_links(vec!["x".to_string()], 2)
            .await
            .unwrap();
        assert_eq!(affected, vec!["x".to_string()]);
    }
}
