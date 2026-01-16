use chrono::{DateTime, Utc};
use serde::Serialize;

use super::clicks::ClickInfo;
use super::stats_list::PaginationMeta;

#[derive(Debug, Serialize)]
pub struct StatsResponse {
    pub code: String,
    pub long_url: String,
    pub created_at: DateTime<Utc>,
    pub total_clicks: i64,
    pub recent_clicks: Vec<ClickInfo>,
    pub pagination: PaginationMeta,
}
