#[derive(Debug, Clone)]
pub struct ClickEvent {
    pub link_id: i64,
    pub user_agent: Option<String>,
    pub referer: Option<String>,
    pub ip: Option<String>,
}
