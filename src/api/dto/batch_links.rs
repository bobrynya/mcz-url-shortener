//! DTOs for the bulk link deactivate/restore endpoints.

use serde::{Deserialize, Serialize};
use validator::Validate;

/// Request body for `POST /api/links/batch-deactivate` and
/// `POST /api/links/batch-restore`.
#[derive(Debug, Deserialize, Validate)]
pub struct BatchLinksRequest {
    /// Short codes to act on. 1..=1000 items; duplicates are de-duplicated.
    #[validate(length(min = 1, max = 1000, message = "codes must contain 1..=1000 items"))]
    pub codes: Vec<String>,

    /// Target domain. When omitted, the default domain is used.
    pub domain_id: Option<i64>,
}

/// Per-code outcome.
#[derive(Debug, Serialize)]
pub struct BatchLinkItem {
    pub code: String,
    /// "deactivated" | "restored" | "not_found".
    pub status: String,
}

/// Summary for batch-deactivate.
#[derive(Debug, Serialize)]
pub struct BatchDeactivateSummary {
    pub total: usize,
    pub deactivated: usize,
    pub not_found: usize,
}

/// Response for `POST /api/links/batch-deactivate`.
#[derive(Debug, Serialize)]
pub struct BatchDeactivateResponse {
    pub summary: BatchDeactivateSummary,
    pub items: Vec<BatchLinkItem>,
}

/// Summary for batch-restore.
#[derive(Debug, Serialize)]
pub struct BatchRestoreSummary {
    pub total: usize,
    pub restored: usize,
    pub not_found: usize,
}

/// Response for `POST /api/links/batch-restore`.
#[derive(Debug, Serialize)]
pub struct BatchRestoreResponse {
    pub summary: BatchRestoreSummary,
    pub items: Vec<BatchLinkItem>,
}
