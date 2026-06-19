//! Value types for click statistics and analytics.

use crate::domain::entities::Click;
use chrono::{DateTime, Utc};

/// Aggregated statistics for a single link.
///
/// Combines link metadata with total click count.
#[derive(Debug, Clone)]
pub struct LinkStats {
    #[allow(dead_code)]
    pub link_id: i64,
    pub code: String,
    pub domain: Option<String>,
    pub long_url: String,
    pub total: i64,
    pub created_at: DateTime<Utc>,
}

/// Detailed statistics with individual click records.
///
/// Includes full link information, total count, and paginated click events.
#[derive(Debug, Clone)]
pub struct DetailedStats {
    pub link: crate::domain::entities::Link,
    pub total: i64,
    pub items: Vec<Click>,
}

/// Filter criteria for statistics queries.
///
/// Supports date range filtering, pagination, and domain scoping.
#[derive(Debug, Clone)]
pub struct StatsFilter {
    pub from_date: Option<DateTime<Utc>>,
    pub to_date: Option<DateTime<Utc>>,
    pub offset: i64,
    pub limit: i64,
    pub domain_id: Option<i64>,
}

impl StatsFilter {
    /// Creates a new filter with pagination parameters.
    pub fn new(offset: i64, limit: i64) -> Self {
        Self {
            from_date: None,
            to_date: None,
            offset,
            limit,
            domain_id: None,
        }
    }

    /// Adds domain filtering to the query.
    pub fn with_domain(mut self, domain_id: Option<i64>) -> Self {
        self.domain_id = domain_id;
        self
    }

    /// Adds date range filtering to the query.
    pub fn with_date_range(
        mut self,
        from_date: Option<DateTime<Utc>>,
        to_date: Option<DateTime<Utc>>,
    ) -> Self {
        self.from_date = from_date;
        self.to_date = to_date;
        self
    }
}
