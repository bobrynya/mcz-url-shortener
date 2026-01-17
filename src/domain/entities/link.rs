use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct Link {
    pub id: i64,
    pub code: String,
    pub long_url: String,
    pub domain: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl Link {
    pub fn new(
        id: i64,
        code: String,
        long_url: String,
        domain: Option<String>,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            code,
            long_url,
            domain,
            created_at,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NewLink {
    pub code: String,
    pub long_url: String,
    pub domain_id: i64,
}
