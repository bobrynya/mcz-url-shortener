#[derive(Debug, Clone)]
pub struct ClickEvent {
    pub link_id: i64,
    pub user_agent: Option<String>,
    pub referer: Option<String>,
    pub ip: Option<String>,
}

impl ClickEvent {
    /// Создать новое событие клика
    pub fn new(
        link_id: i64,
        ip: Option<String>,
        user_agent: Option<&str>,
        referer: Option<&str>,
    ) -> Self {
        Self {
            link_id,
            ip,
            user_agent: user_agent.map(|s| s.to_string()),
            referer: referer.map(|s| s.to_string()),
        }
    }
}
