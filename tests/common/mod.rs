#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use sqlx::PgPool;
use url_shortener::domain::click_event::ClickEvent;
use url_shortener::domain::entities::Click;
use url_shortener::domain::repositories::{ClickPublisher, ClickStatsReader, StatsFilter};
use url_shortener::error::AppError;
use url_shortener::infrastructure::cache::NullCache;
use url_shortener::infrastructure::messaging::NoopClickPublisher;
use url_shortener::infrastructure::persistence::{
    PgDomainRepository, PgLinkRepository, PgTokenRepository, UnavailableStatsReader,
};
use url_shortener::state::AppState;

pub async fn create_test_domain(pool: &PgPool, name: &str) -> i64 {
    sqlx::query_scalar!(
        "INSERT INTO domains (domain, is_default) VALUES ($1, false) RETURNING id",
        name
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

pub async fn get_default_domain(pool: &PgPool) -> i64 {
    sqlx::query_scalar!("SELECT id FROM domains WHERE is_default = true LIMIT 1")
        .fetch_one(pool)
        .await
        .unwrap()
}

pub async fn create_test_link(pool: &PgPool, code: &str, url: &str, domain_id: i64) {
    sqlx::query!(
        "INSERT INTO links (code, long_url, domain_id) VALUES ($1, $2, $3)",
        code,
        url,
        domain_id
    )
    .execute(pool)
    .await
    .unwrap();
}

pub async fn create_deleted_link(pool: &PgPool, code: &str, url: &str, domain_id: i64) {
    sqlx::query!(
        "INSERT INTO links (code, long_url, domain_id, deleted_at) VALUES ($1, $2, $3, NOW())",
        code,
        url,
        domain_id
    )
    .execute(pool)
    .await
    .unwrap();
}

pub async fn create_expired_link(pool: &PgPool, code: &str, url: &str, domain_id: i64) {
    sqlx::query!(
        "INSERT INTO links (code, long_url, domain_id, expires_at) VALUES ($1, $2, $3, NOW() - INTERVAL '1 hour')",
        code,
        url,
        domain_id
    )
    .execute(pool)
    .await
    .unwrap();
}

pub async fn create_permanent_link(pool: &PgPool, code: &str, url: &str, domain_id: i64) {
    sqlx::query!(
        "INSERT INTO links (code, long_url, domain_id, permanent) VALUES ($1, $2, $3, TRUE)",
        code,
        url,
        domain_id
    )
    .execute(pool)
    .await
    .unwrap();
}

/// A [`ClickPublisher`] that records every published event for later inspection.
///
/// Integration tests can't use the `#[cfg(test)]`-gated `MockClickPublisher`
/// (it isn't compiled into the library for external crates), so this provides an
/// equivalent recording double for asserting redirect publish behaviour.
#[derive(Clone, Default)]
pub struct RecordingClickPublisher {
    events: Arc<Mutex<Vec<ClickEvent>>>,
}

impl RecordingClickPublisher {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a snapshot of all events published so far.
    pub fn events(&self) -> Vec<ClickEvent> {
        self.events.lock().unwrap().clone()
    }
}

#[async_trait]
impl ClickPublisher for RecordingClickPublisher {
    async fn publish(&self, event: ClickEvent) -> Result<(), AppError> {
        self.events.lock().unwrap().push(event);
        Ok(())
    }
}

/// A [`ClickStatsReader`] returning canned per-link click data.
///
/// Lets stats-handler tests exercise the read path without a live ClickHouse.
#[derive(Clone, Default)]
pub struct FakeStatsReader {
    counts: HashMap<i64, i64>,
    clicks: HashMap<i64, Vec<Click>>,
}

impl FakeStatsReader {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the total click count and click list for a given link id.
    pub fn with_link(mut self, link_id: i64, clicks: Vec<Click>) -> Self {
        self.counts.insert(link_id, clicks.len() as i64);
        self.clicks.insert(link_id, clicks);
        self
    }
}

#[async_trait]
impl ClickStatsReader for FakeStatsReader {
    async fn count_clicks(&self, link_id: i64, _filter: &StatsFilter) -> Result<i64, AppError> {
        Ok(self.counts.get(&link_id).copied().unwrap_or(0))
    }

    async fn list_clicks(
        &self,
        link_id: i64,
        _filter: &StatsFilter,
    ) -> Result<Vec<Click>, AppError> {
        Ok(self.clicks.get(&link_id).cloned().unwrap_or_default())
    }

    async fn counts_for_links(
        &self,
        link_ids: &[i64],
        _filter: &StatsFilter,
    ) -> Result<HashMap<i64, i64>, AppError> {
        Ok(link_ids
            .iter()
            .filter_map(|id| self.counts.get(id).map(|c| (*id, *c)))
            .collect())
    }
}

/// Builds an [`AppState`] for tests with the given click publisher and stats reader.
///
/// Kafka/ClickHouse health handles are left `None` (unconfigured), so health
/// checks report them as non-critical "error" — exactly the production behaviour
/// when those backends are absent.
pub fn create_test_state_with(
    pool: PgPool,
    click_publisher: Arc<dyn ClickPublisher>,
    stats_reader: Arc<dyn ClickStatsReader>,
) -> AppState {
    let pool = Arc::new(pool);

    let link_repo = Arc::new(PgLinkRepository::new(pool.clone()));
    let domain_repo = Arc::new(PgDomainRepository::new(pool.clone()));
    let token_repo = Arc::new(PgTokenRepository::new(pool.clone()));

    AppState::new(
        link_repo,
        token_repo,
        domain_repo,
        click_publisher,
        stats_reader,
        None,
        None,
        Arc::new(NullCache),
        "test-signing-secret".to_string(),
        false,
        // Integration tests exercise public URLs; mirror the production default.
        true,
    )
}

/// Convenience builder: no-op click publisher + unavailable stats reader.
///
/// Suitable for tests that don't inspect published clicks or read stats.
pub fn create_test_state(pool: PgPool) -> AppState {
    create_test_state_with(
        pool,
        Arc::new(NoopClickPublisher),
        Arc::new(UnavailableStatsReader),
    )
}
