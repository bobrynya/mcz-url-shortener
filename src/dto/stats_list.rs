use crate::dto::stats::StatsResponse;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct StatsListQuery {
    pub page: Option<u32>,
    pub page_size: Option<u32>,

    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
pub struct StatsListResponse {
    pub page: u32,
    pub page_size: u32,
    pub total: i64,
    pub items: Vec<StatsResponse>,
}
