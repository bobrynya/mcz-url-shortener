use crate::error::ErrorInfo;
use regex::Regex;
use serde::{Deserialize, Serialize};
use validator::Validate;

lazy_static::lazy_static! {
    static ref CUSTOM_CODE_REGEX: Regex =
        Regex::new(r"^[a-z0-9-]+$").unwrap();
}

#[derive(Debug, Deserialize, Validate)]
pub struct ShortenRequest {
    #[validate(nested)]
    pub urls: Vec<UrlItem>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UrlItem {
    #[validate(url(message = "Invalid URL format"))]
    pub url: String,

    pub domain: Option<String>,

    #[validate(length(min = 3, max = 20))]
    #[validate(regex(path = "*CUSTOM_CODE_REGEX"))]
    pub custom_code: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ShortenResponse {
    pub summary: BatchSummary,
    pub items: Vec<ShortenResultItem>,
}

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

#[derive(Debug, Serialize)]
pub struct BatchSummary {
    pub total: usize,
    pub successful: usize,
    pub failed: usize,
}
