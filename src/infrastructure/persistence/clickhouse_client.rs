//! Reconnecting ClickHouse client, row mapping, and insert sink.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use clickhouse::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::domain::click_event::ClickEvent;
use crate::error::AppError;

/// A click row as stored in ClickHouse (`url_shortener.clicks`).
#[derive(Debug, Clone, clickhouse::Row, Serialize, Deserialize)]
pub struct ClickRow {
    pub link_id: u64,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    pub referer: Option<String>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub clicked_at: DateTime<Utc>,
}

impl From<&ClickEvent> for ClickRow {
    fn from(e: &ClickEvent) -> Self {
        Self {
            // link_id is a positive bigint from Postgres; cast to UInt64.
            link_id: e.link_id.max(0) as u64,
            ip: e.ip.clone(),
            user_agent: e.user_agent.clone(),
            referer: e.referer.clone(),
            clicked_at: e.clicked_at,
        }
    }
}

/// Connection settings for ClickHouse (HTTP interface).
#[derive(Debug, Clone)]
pub struct ClickHouseConfig {
    pub url: String,
    pub database: String,
    pub user: String,
    pub password: String,
    pub retry_interval: Duration,
}

/// Insert abstraction so the consumer's batching is testable without a real DB.
#[async_trait]
pub trait ClickSink: Send + Sync {
    /// Inserts a batch of click rows. Returns `Err` only on a real failure
    /// (caller must NOT commit Kafka offsets on error).
    async fn insert_batch(&self, rows: &[ClickRow]) -> Result<(), AppError>;
}

/// ClickHouse client with lazy connection and a retry cooldown.
///
/// If ClickHouse is down at startup the service keeps serving redirects; reads
/// return 503 and inserts fail (offsets stay uncommitted) until it recovers.
pub struct ReconnectingClickHouse {
    cfg: ClickHouseConfig,
    client: Mutex<Option<Client>>,
    next_attempt_at: Mutex<Instant>,
}

impl ReconnectingClickHouse {
    pub fn new(cfg: ClickHouseConfig) -> Self {
        Self {
            cfg,
            client: Mutex::new(None),
            next_attempt_at: Mutex::new(Instant::now()),
        }
    }

    fn build_client(&self) -> Client {
        Client::default()
            .with_url(&self.cfg.url)
            .with_database(&self.cfg.database)
            .with_user(&self.cfg.user)
            .with_password(&self.cfg.password)
    }

    /// Returns a connected client, or `None` while ClickHouse is unavailable.
    pub async fn get(&self) -> Option<Client> {
        {
            let guard = self.client.lock().await;
            if let Some(c) = guard.as_ref() {
                return Some(c.clone());
            }
        }

        let mut next = self.next_attempt_at.lock().await;
        if Instant::now() < *next {
            return None;
        }
        *next = Instant::now() + self.cfg.retry_interval;

        let candidate = self.build_client();
        match candidate.query("SELECT 1").execute().await {
            Ok(()) => {
                let mut guard = self.client.lock().await;
                *guard = Some(candidate.clone());
                tracing::info!("ClickHouse connected");
                Some(candidate)
            }
            Err(e) => {
                tracing::warn!(error = %e, "ClickHouse unavailable");
                None
            }
        }
    }

    /// Drops the cached client so the next `get()` reconnects.
    async fn invalidate(&self) {
        *self.client.lock().await = None;
    }

    /// Health probe used by `/health` (non-critical).
    pub async fn health_check(&self) -> bool {
        self.get().await.is_some()
    }
}

#[async_trait]
impl ClickSink for ReconnectingClickHouse {
    async fn insert_batch(&self, rows: &[ClickRow]) -> Result<(), AppError> {
        if rows.is_empty() {
            return Ok(());
        }
        let client = self.get().await.ok_or_else(|| {
            AppError::service_unavailable("ClickHouse is unavailable", serde_json::json!({}))
        })?;

        let mut insert = client.insert("clicks").map_err(|e| {
            AppError::service_unavailable(
                "ClickHouse insert failed",
                serde_json::json!({ "error": e.to_string() }),
            )
        })?;
        for row in rows {
            // write() errors are local (row encoding/buffering on the not-yet-sent HTTP request);
            // the connection is still healthy. Only end() failure (commit) signals a real connection issue.
            insert.write(row).await.map_err(|e| {
                AppError::service_unavailable(
                    "ClickHouse write failed",
                    serde_json::json!({ "error": e.to_string() }),
                )
            })?;
        }
        match insert.end().await {
            Ok(()) => Ok(()),
            Err(e) => {
                self.invalidate().await;
                Err(AppError::service_unavailable(
                    "ClickHouse commit failed",
                    serde_json::json!({ "error": e.to_string() }),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_click_row_from_event() {
        let e = ClickEvent::new(
            99,
            Some("1.2.3.4".to_string()),
            Some("UA".to_string()),
            None,
            Utc.with_ymd_and_hms(2026, 6, 19, 0, 0, 0).unwrap(),
        );
        let row = ClickRow::from(&e);
        assert_eq!(row.link_id, 99);
        assert_eq!(row.ip.as_deref(), Some("1.2.3.4"));
        assert!(row.referer.is_none());
    }
}
