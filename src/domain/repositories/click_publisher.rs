//! Write-side port for publishing click events (Kafka).

use async_trait::async_trait;

use crate::domain::click_event::ClickEvent;
use crate::error::AppError;

/// Publishes click events to the messaging backbone.
///
/// Implementations are fire-and-forget from the caller's perspective: when the
/// backend is unavailable the click is dropped (logged + metered) and `Ok(())`
/// is returned so the redirect never blocks or fails.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait ClickPublisher: Send + Sync {
    /// Publishes a single click event.
    async fn publish(&self, event: ClickEvent) -> Result<(), AppError>;
}
