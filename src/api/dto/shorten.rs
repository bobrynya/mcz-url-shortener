//! DTOs for link shortening endpoint.

use crate::error::ErrorInfo;
use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use validator::Validate;

/// Compiled regex for custom code validation.
static CUSTOM_CODE_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z0-9-]+$").unwrap());

/// Request to shorten one or more URLs.
///
/// Supports batch processing for efficiency when creating multiple links.
#[derive(Debug, Deserialize, Validate)]
pub struct ShortenRequest {
    #[validate(nested)]
    pub urls: Vec<UrlItem>,
}

/// Individual URL to be shortened.
#[derive(Debug, Deserialize, Validate)]
pub struct UrlItem {
    /// The original URL to shorten (must be valid HTTP/HTTPS).
    #[validate(url(message = "Invalid URL format"))]
    pub url: String,

    /// Optional target domain id (otherwise uses the default domain).
    pub domain_id: Option<i64>,

    /// Optional custom short code (validated for length and characters).
    #[validate(length(min = 4, max = 50))]
    #[validate(regex(path = "*CUSTOM_CODE_REGEX"))]
    pub custom_code: Option<String>,

    /// Optional expiry timestamp. After this time, the link returns 410 Gone.
    pub expires_at: Option<DateTime<Utc>>,

    /// When true, uses 301 Permanent Redirect instead of 307 Temporary.
    pub permanent: Option<bool>,
}

/// Response containing batch processing results.
#[derive(Debug, Serialize)]
pub struct ShortenResponse {
    pub summary: BatchSummary,
    pub items: Vec<ShortenResultItem>,
}

/// Individual result for a URL in the batch.
///
/// Uses untagged enum for cleaner JSON structure (no discriminator field).
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum ShortenResultItem {
    Success {
        long_url: String,
        code: String,
        short_url: String,
    },
    Error {
        long_url: String,
        error: ErrorInfo,
    },
}

/// Summary statistics for batch processing.
#[derive(Debug, Serialize)]
pub struct BatchSummary {
    pub total: usize,
    pub successful: usize,
    pub failed: usize,
}
