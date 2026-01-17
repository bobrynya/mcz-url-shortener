use chrono::{DateTime, Utc};
use serde::Serialize;

/// Список всех ссылок со статистикой
#[derive(Debug, Serialize)]
pub struct StatsListResponse {
    pub pagination: PaginationMeta,
    pub items: Vec<LinkStatsItem>,
}

#[derive(Debug, Serialize)]
pub struct LinkStatsItem {
    pub code: String,
    pub domain: Option<String>,
    pub long_url: String,
    pub total: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct PaginationMeta {
    pub page: u32,
    pub page_size: u32,
    pub total_items: i64,
    pub total_pages: u32,
}
