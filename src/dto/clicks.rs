use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Serialize)]
pub struct ClickItem {
    pub id: i64,
    pub clicked_at: DateTime<Utc>,
    pub referer: Option<String>,
    pub user_agent: Option<String>,
    pub ip: Option<String>,
}

#[derive(Serialize)]
pub struct ClicksResponse {
    pub code: String,
    pub page: u32,
    pub page_size: u32,
    pub total: i64,
    pub items: Vec<ClickItem>,
}
