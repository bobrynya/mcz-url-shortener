//! Repository trait for short link data access.

use crate::domain::entities::{Link, LinkPatch, NewLink};
use crate::error::AppError;
use async_trait::async_trait;

/// Repository interface for managing short links.
///
/// Provides CRUD operations for shortened URLs, including lookups by code,
/// long URL, and pagination support.
///
/// # Implementations
///
/// - [`crate::infrastructure::persistence::PgLinkRepository`] - PostgreSQL implementation
/// - Test mocks available with `cfg(test)`
///
/// # Examples
///
/// See integration tests: `tests/repository_link.rs`
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait LinkRepository: Send + Sync {
    /// Creates a new short link.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Conflict`] if:
    /// - The short code already exists for the given domain
    /// - The long URL is already shortened for the given domain
    ///
    /// Returns [`AppError::Internal`] on database errors.
    async fn create(&self, new_link: NewLink) -> Result<Link, AppError>;

    /// Finds a link by its short code and domain.
    ///
    /// # Returns
    ///
    /// - `Ok(Some(Link))` if found
    /// - `Ok(None)` if not found
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Internal`] on database errors.
    async fn find_by_code(&self, code: &str, domain_id: i64) -> Result<Option<Link>, AppError>;

    /// Finds a link by code across all domains (used by stats when no domain filter).
    ///
    /// # Errors
    /// Returns [`AppError::Internal`] on database errors.
    async fn find_any_by_code(&self, code: &str) -> Result<Option<Link>, AppError>;

    /// Finds a link by its original long URL and domain.
    ///
    /// Used to check if a URL has already been shortened for a specific domain.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Internal`] on database errors.
    async fn find_by_long_url(
        &self,
        long_url: &str,
        domain_id: i64,
    ) -> Result<Option<Link>, AppError>;

    /// Lists links with pagination support.
    ///
    /// # Arguments
    ///
    /// - `page` - Page number (1-indexed)
    /// - `page_size` - Number of items per page
    /// - `domain_id` - Optional domain filter
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Internal`] on database errors.
    async fn list(
        &self,
        page: i64,
        page_size: i64,
        domain_id: Option<i64>,
    ) -> Result<Vec<Link>, AppError>;

    /// Counts total links, optionally filtered by domain.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Internal`] on database errors.
    async fn count(&self, domain_id: Option<i64>) -> Result<i64, AppError>;

    /// Soft-deletes a link by setting `deleted_at = now()`.
    ///
    /// Returns `Ok(true)` if the link was found and deleted, `Ok(false)` if not found
    /// or already deleted.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Internal`] on database errors.
    async fn soft_delete(&self, code: &str, domain_id: i64) -> Result<bool, AppError>;

    /// Partially updates a link.
    ///
    /// Only fields present in [`LinkPatch`] are modified. `None` fields are unchanged.
    /// When `patch.restore` is `true`, `deleted_at` is cleared.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::NotFound`] if no link matches `code` + `domain_id`.
    /// Returns [`AppError::Internal`] on database errors.
    async fn update(&self, code: &str, domain_id: i64, patch: LinkPatch) -> Result<Link, AppError>;

    /// Deactivates (soft-deletes) the given codes within a domain in one
    /// statement. Only currently-active links are transitioned. Returns the
    /// codes that were actually transitioned, so callers can compute which
    /// inputs were `not_found` (absent or already deleted).
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Internal`] on database errors.
    async fn soft_delete_many(
        &self,
        codes: &[String],
        domain_id: i64,
    ) -> Result<Vec<String>, AppError>;

    /// Restores the given soft-deleted codes within a domain in one statement.
    /// Only currently-deleted links are transitioned. Returns the codes that
    /// were actually transitioned.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Internal`] on database errors.
    async fn restore_many(&self, codes: &[String], domain_id: i64)
    -> Result<Vec<String>, AppError>;
}
