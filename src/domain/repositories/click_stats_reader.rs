//! Read-side port for click statistics (ClickHouse).

use std::collections::HashMap;

use async_trait::async_trait;

use crate::domain::entities::Click;
use crate::domain::repositories::StatsFilter;
use crate::error::AppError;

/// Reads click analytics from the columnar store.
///
/// All methods return [`AppError::ServiceUnavailable`] when the store is down,
/// which surfaces to clients as HTTP 503.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait ClickStatsReader: Send + Sync {
    /// Counts clicks for one link within the filter's date range.
    async fn count_clicks(&self, link_id: i64, filter: &StatsFilter) -> Result<i64, AppError>;

    /// Lists paginated click records for one link, newest first.
    async fn list_clicks(&self, link_id: i64, filter: &StatsFilter)
    -> Result<Vec<Click>, AppError>;

    /// Returns per-link click totals for a set of links (used by `get_all_stats`).
    /// Links with no clicks are simply absent from the map.
    async fn counts_for_links(
        &self,
        link_ids: &[i64],
        filter: &StatsFilter,
    ) -> Result<HashMap<i64, i64>, AppError>;
}
