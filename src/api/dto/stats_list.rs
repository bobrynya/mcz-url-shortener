use chrono::{DateTime, Utc};
use serde::Serialize;

/// Список всех ссылок со статистикой
#[derive(Debug, Serialize)]
pub struct StatsListResponse {
    pub items: Vec<LinkStatsItem>,
    pub pagination: PaginationMeta,
}

#[derive(Debug, Serialize)]
pub struct LinkStatsItem {
    pub code: String,
    pub long_url: String,
    pub total_clicks: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct PaginationMeta {
    pub page: u32,
    pub page_size: u32,
    pub total_items: i64,
    pub total_pages: u32,
}
