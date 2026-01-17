use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct Domain {
    pub id: i64,
    pub domain: String,
    pub is_default: bool,
    pub is_active: bool,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Domain {
    pub fn new(
        id: i64,
        domain: String,
        is_default: bool,
        is_active: bool,
        description: Option<String>,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            domain,
            is_default,
            is_active,
            description,
            created_at,
            updated_at,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NewDomain {
    pub domain: String,
    pub is_default: bool,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct UpdateDomain {
    pub is_active: Option<bool>,
    pub description: Option<String>,
}
