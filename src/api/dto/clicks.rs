use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ClickInfo {
    pub clicked_at: DateTime<Utc>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub referer: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
}
