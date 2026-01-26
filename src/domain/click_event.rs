#[derive(Debug, Clone)]
pub struct ClickEvent {
    pub domain: String,
    pub code: String,
    pub user_agent: Option<String>,
    pub referer: Option<String>,
    pub ip: Option<String>,
}

impl ClickEvent {
    /// Создать новое событие клика
    pub fn new(
        domain: String,
        code: String,
        ip: Option<String>,
        user_agent: Option<&str>,
        referer: Option<&str>,
    ) -> Self {
        Self {
            domain,
            code,
            ip,
            user_agent: user_agent.map(|s| s.to_string()),
            referer: referer.map(|s| s.to_string()),
        }
    }
}
