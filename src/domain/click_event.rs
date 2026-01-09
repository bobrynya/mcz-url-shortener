use chrono::{DateTime, Utc};
use std::net::IpAddr;

#[derive(Debug, Clone)]
pub struct ClickEvent {
    pub link_id: i64,
    pub clicked_at: DateTime<Utc>,
    pub referer: Option<String>,
    pub user_agent: Option<String>,
    pub ip: Option<IpAddr>,
}
