//! ClickHouse implementation of the click statistics read port.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::domain::entities::Click;
use crate::domain::repositories::{ClickStatsReader, StatsFilter};
use crate::error::AppError;
use crate::infrastructure::persistence::ReconnectingClickHouse;

/// Reader used when ClickHouse is not configured — every call is a 503.
pub struct UnavailableStatsReader;

#[async_trait]
impl ClickStatsReader for UnavailableStatsReader {
    async fn count_clicks(&self, _link_id: i64, _filter: &StatsFilter) -> Result<i64, AppError> {
        Err(unavailable())
    }
    async fn list_clicks(
        &self,
        _link_id: i64,
        _filter: &StatsFilter,
    ) -> Result<Vec<Click>, AppError> {
        Err(unavailable())
    }
    async fn counts_for_links(
        &self,
        _link_ids: &[i64],
        _filter: &StatsFilter,
    ) -> Result<HashMap<i64, i64>, AppError> {
        Err(unavailable())
    }
}

fn unavailable() -> AppError {
    AppError::service_unavailable("ClickHouse is not configured", serde_json::json!({}))
}

/// Bounds used to translate `Option` date filters into concrete ClickHouse params.
const MIN_TS: &str = "1970-01-01 00:00:00.000";
const MAX_TS: &str = "9999-01-01 00:00:00.000";

fn from_bound(f: &StatsFilter) -> String {
    f.from_date
        .map(fmt_ts)
        .unwrap_or_else(|| MIN_TS.to_string())
}
fn to_bound(f: &StatsFilter) -> String {
    f.to_date.map(fmt_ts).unwrap_or_else(|| MAX_TS.to_string())
}
fn fmt_ts(ts: DateTime<Utc>) -> String {
    ts.format("%Y-%m-%d %H:%M:%S%.3f").to_string()
}

/// Row returned when reading individual clicks.
#[derive(Debug, Deserialize, clickhouse::Row)]
struct ClickReadRow {
    link_id: u64,
    ip: Option<String>,
    user_agent: Option<String>,
    referer: Option<String>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    clicked_at: DateTime<Utc>,
}

/// Row returned when aggregating per-link counts.
#[derive(Debug, Deserialize, clickhouse::Row)]
struct CountRow {
    link_id: u64,
    total: u64,
}

/// ClickHouse-backed implementation of [`ClickStatsReader`].
pub struct ClickHouseStatsReader {
    ch: Arc<ReconnectingClickHouse>,
}

impl ClickHouseStatsReader {
    pub fn new(ch: Arc<ReconnectingClickHouse>) -> Self {
        Self { ch }
    }

    async fn client(&self) -> Result<clickhouse::Client, AppError> {
        self.ch.get().await.ok_or_else(|| {
            AppError::service_unavailable("ClickHouse is unavailable", serde_json::json!({}))
        })
    }
}

#[async_trait]
impl ClickStatsReader for ClickHouseStatsReader {
    async fn count_clicks(&self, link_id: i64, filter: &StatsFilter) -> Result<i64, AppError> {
        let client = self.client().await?;
        let count: u64 = client
            .query(
                "SELECT count() FROM clicks \
                 WHERE link_id = ? AND clicked_at >= ? AND clicked_at <= ?",
            )
            .bind(link_id.max(0) as u64)
            .bind(from_bound(filter))
            .bind(to_bound(filter))
            .fetch_one()
            .await
            .map_err(map_ch_err)?;
        Ok(count as i64)
    }

    async fn list_clicks(
        &self,
        link_id: i64,
        filter: &StatsFilter,
    ) -> Result<Vec<Click>, AppError> {
        let client = self.client().await?;
        let rows: Vec<ClickReadRow> = client
            .query(
                "SELECT link_id, ip, user_agent, referer, clicked_at FROM clicks \
                 WHERE link_id = ? AND clicked_at >= ? AND clicked_at <= ? \
                 ORDER BY clicked_at DESC LIMIT ? OFFSET ?",
            )
            .bind(link_id.max(0) as u64)
            .bind(from_bound(filter))
            .bind(to_bound(filter))
            .bind(filter.limit.max(0) as u64)
            .bind(filter.offset.max(0) as u64)
            .fetch_all()
            .await
            .map_err(map_ch_err)?;

        Ok(rows
            .into_iter()
            .map(|r| {
                Click::new(
                    0,
                    r.link_id as i64,
                    r.clicked_at,
                    r.user_agent,
                    r.referer,
                    r.ip,
                )
            })
            .collect())
    }

    async fn counts_for_links(
        &self,
        link_ids: &[i64],
        filter: &StatsFilter,
    ) -> Result<HashMap<i64, i64>, AppError> {
        if link_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let client = self.client().await?;
        let ids: Vec<u64> = link_ids.iter().map(|id| (*id).max(0) as u64).collect();
        let rows: Vec<CountRow> = client
            .query(
                "SELECT link_id, count() AS total FROM clicks \
                 WHERE link_id IN ? AND clicked_at >= ? AND clicked_at <= ? \
                 GROUP BY link_id",
            )
            .bind(ids)
            .bind(from_bound(filter))
            .bind(to_bound(filter))
            .fetch_all()
            .await
            .map_err(map_ch_err)?;

        Ok(rows
            .into_iter()
            .map(|r| (r.link_id as i64, r.total as i64))
            .collect())
    }
}

fn map_ch_err(e: clickhouse::error::Error) -> AppError {
    tracing::warn!(error = %e, "ClickHouse query failed");
    AppError::service_unavailable("ClickHouse query failed", serde_json::json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_unavailable_reader_returns_503() {
        let r = UnavailableStatsReader;
        let f = StatsFilter::new(0, 10);
        let err = r.count_clicks(1, &f).await.unwrap_err();
        assert!(matches!(err, AppError::ServiceUnavailable { .. }));
    }
}
