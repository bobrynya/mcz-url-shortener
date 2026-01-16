use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ShortenRequest {
    Single { url: String },
    Batch { urls: Vec<String> },
}

impl ShortenRequest {
    pub fn into_urls(self) -> Vec<String> {
        match self {
            ShortenRequest::Single { url } => vec![url],
            ShortenRequest::Batch { urls } => urls,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ShortenResponse {
    pub items: Vec<ShortenedLinkItem>,
}

#[derive(Debug, Serialize)]
pub struct ShortenedLinkItem {
    pub long_url: String,
    pub code: String,
    pub short_url: String,
}
