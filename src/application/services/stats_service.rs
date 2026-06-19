//! Click statistics service: merges Postgres link metadata with ClickHouse analytics.

use std::sync::Arc;

use serde_json::json;

use crate::domain::repositories::{
    ClickStatsReader, DetailedStats, LinkRepository, LinkStats, StatsFilter,
};
use crate::error::AppError;

/// Service for retrieving click statistics and analytics.
///
/// Link metadata is read from Postgres via [`LinkRepository`]; click counts and
/// individual records come from ClickHouse via [`ClickStatsReader`]. The reader is
/// held as a trait object so the configured/unconfigured fallback is uniform.
pub struct StatsService<L: LinkRepository> {
    reader: Arc<dyn ClickStatsReader>,
    links: Arc<L>,
}

impl<L: LinkRepository> StatsService<L> {
    /// Creates a new statistics service.
    pub fn new(reader: Arc<dyn ClickStatsReader>, links: Arc<L>) -> Self {
        Self { reader, links }
    }

    /// Detailed stats for one short code: metadata from PG, clicks from ClickHouse.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::NotFound`] if no link matches the code.
    /// Returns [`AppError::ServiceUnavailable`] if ClickHouse is down.
    pub async fn get_detailed_stats(
        &self,
        code: &str,
        filter: StatsFilter,
    ) -> Result<DetailedStats, AppError> {
        let link = self
            .links
            .find_by_code(code, filter.domain_id.unwrap_or(0))
            .await?;

        // When no domain filter is supplied we still need to resolve by code alone;
        // find_by_code requires a domain_id, so fall back to a code-only lookup.
        let link = match link {
            Some(l) => l,
            None if filter.domain_id.is_none() => {
                self.links.find_any_by_code(code).await?.ok_or_else(|| {
                    AppError::not_found("Statistics not found", json!({ "code": code }))
                })?
            }
            None => {
                return Err(AppError::not_found(
                    "Statistics not found",
                    json!({ "code": code }),
                ));
            }
        };

        let total = self.reader.count_clicks(link.id, &filter).await?;
        let items = self.reader.list_clicks(link.id, &filter).await?;

        Ok(DetailedStats { link, total, items })
    }

    /// Aggregated per-link stats: page links from PG, counts from ClickHouse, merge.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::ServiceUnavailable`] if ClickHouse is down.
    /// Returns [`AppError::Internal`] on database errors.
    pub async fn get_all_stats(&self, filter: StatsFilter) -> Result<Vec<LinkStats>, AppError> {
        let page = (filter.offset / filter.limit.max(1)) + 1;
        let links = self
            .links
            .list(page, filter.limit, filter.domain_id)
            .await?;

        let ids: Vec<i64> = links.iter().map(|l| l.id).collect();
        let counts = self.reader.counts_for_links(&ids, &filter).await?;

        Ok(links
            .into_iter()
            .map(|l| LinkStats {
                link_id: l.id,
                code: l.code,
                domain: l.domain,
                long_url: l.long_url,
                total: counts.get(&l.id).copied().unwrap_or(0),
                created_at: l.created_at,
            })
            .collect())
    }

    /// Total link count (Postgres), for pagination metadata.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Internal`] on database errors.
    pub async fn count_all_links(&self) -> Result<i64, AppError> {
        self.links.count(None).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::Link;
    use crate::domain::repositories::{MockClickStatsReader, MockLinkRepository};
    use chrono::Utc;
    use std::collections::HashMap;

    fn link(id: i64, code: &str) -> Link {
        Link::new(
            id,
            code.to_string(),
            "https://example.com".to_string(),
            Some("s.example.com".to_string()),
            Utc::now(),
            None,
            false,
            None,
        )
    }

    #[tokio::test]
    async fn test_get_all_stats_merges_counts_and_defaults_zero() {
        let mut links = MockLinkRepository::new();
        links
            .expect_list()
            .returning(|_, _, _| Ok(vec![link(1, "aaa"), link(2, "bbb")]));

        let mut reader = MockClickStatsReader::new();
        reader.expect_counts_for_links().returning(|_, _| {
            let mut m = HashMap::new();
            m.insert(1, 10);
            Ok(m) // link 2 absent → should default to 0
        });

        let svc = StatsService::new(
            Arc::new(reader) as Arc<dyn ClickStatsReader>,
            Arc::new(links),
        );
        let out = svc.get_all_stats(StatsFilter::new(0, 25)).await.unwrap();

        assert_eq!(out.len(), 2);
        assert_eq!(out[0].total, 10);
        assert_eq!(out[1].total, 0);
    }

    #[tokio::test]
    async fn test_get_detailed_stats_not_found() {
        let mut links = MockLinkRepository::new();
        links.expect_find_by_code().returning(|_, _| Ok(None));
        links.expect_find_any_by_code().returning(|_| Ok(None));
        let reader = MockClickStatsReader::new();

        let svc = StatsService::new(
            Arc::new(reader) as Arc<dyn ClickStatsReader>,
            Arc::new(links),
        );
        let err = svc
            .get_detailed_stats("missing", StatsFilter::new(0, 25))
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::NotFound { .. }));
    }

    #[tokio::test]
    async fn test_get_detailed_stats_reader_unavailable_propagates_503() {
        let mut links = MockLinkRepository::new();
        links
            .expect_find_by_code()
            .returning(|_, _| Ok(Some(link(1, "aaa"))));
        let mut reader = MockClickStatsReader::new();
        reader
            .expect_count_clicks()
            .returning(|_, _| Err(AppError::service_unavailable("down", serde_json::json!({}))));

        let filter = StatsFilter::new(0, 25).with_domain(Some(1));
        let svc = StatsService::new(
            Arc::new(reader) as Arc<dyn ClickStatsReader>,
            Arc::new(links),
        );
        let err = svc.get_detailed_stats("aaa", filter).await.unwrap_err();
        assert!(matches!(err, AppError::ServiceUnavailable { .. }));
    }
}
