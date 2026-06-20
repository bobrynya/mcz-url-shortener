//! DTO for the link update endpoint.

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_with::serde_as;
use validator::Validate;

/// Request body for `PATCH /api/links/{code}`.
///
/// All fields are optional — only provided fields are changed.
///
/// # `expires_at` semantics
///
/// - **Absent** (`expires_at` not in JSON) → leave existing value unchanged
/// - **`null`** → clear expiry (link never expires)
/// - **Timestamp** → set new expiry
#[serde_as]
#[derive(Debug, Deserialize, Validate)]
pub struct UpdateLinkRequest {
    /// New destination URL for this link.
    #[validate(url(message = "Invalid URL format"))]
    pub url: Option<String>,

    /// Expiry timestamp. Absent = no change, null = clear, value = set.
    #[serde(default, with = "::serde_with::rust::double_option")]
    pub expires_at: Option<Option<DateTime<Utc>>>,

    /// Change redirect type: true = 301 permanent, false = 307 temporary.
    pub permanent: Option<bool>,

    /// When true, clears `deleted_at` to restore a soft-deleted link.
    #[serde(default)]
    pub restore: bool,

    /// Target domain id. When omitted, the default domain is used.
    pub domain_id: Option<i64>,
}
