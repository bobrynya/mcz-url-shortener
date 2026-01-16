use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct Click {
    #[allow(dead_code)]
    pub id: i64,
    #[allow(dead_code)]
    pub link_id: i64,
    pub clicked_at: DateTime<Utc>,
    pub user_agent: Option<String>,
    pub referer: Option<String>,
    pub ip: Option<String>,
}

impl Click {
    pub fn new(
        id: i64,
        link_id: i64,
        clicked_at: DateTime<Utc>,
        user_agent: Option<String>,
        referer: Option<String>,
        ip: Option<String>,
    ) -> Self {
        Self {
            id,
            link_id,
            clicked_at,
            user_agent,
            referer,
            ip,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NewClick {
    pub link_id: i64,
    pub user_agent: Option<String>,
    pub referer: Option<String>,
    pub ip: Option<String>,
}
