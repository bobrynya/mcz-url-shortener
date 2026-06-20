//! Application state shared across HTTP handlers.
//!
//! Contains service instances, database pool, cache, and the click publisher used
//! for asynchronous click tracking. Cloned for each request via Axum's state
//! extraction.

use std::sync::Arc;

use metrics_exporter_prometheus::PrometheusHandle;

use crate::application::services::{AuthService, DomainService, LinkService, StatsService};
use crate::domain::repositories::{ClickPublisher, ClickStatsReader};
use crate::infrastructure::cache::CacheService;
use crate::infrastructure::messaging::KafkaClickPublisher;
use crate::infrastructure::persistence::{
    PgDomainRepository, PgLinkRepository, PgTokenRepository, ReconnectingClickHouse,
};

/// Shared application state injected into HTTP handlers.
///
/// Contains all services and infrastructure components needed to process requests.
/// Cheap to clone due to `Arc` wrapping.
#[derive(Clone)]
pub struct AppState {
    pub link_service: Arc<LinkService<PgLinkRepository, PgDomainRepository>>,
    pub stats_service: Arc<StatsService<PgLinkRepository>>,
    pub auth_service: Arc<AuthService<PgTokenRepository>>,
    pub domain_service: Arc<DomainService<PgDomainRepository>>,

    pub cache: Arc<dyn CacheService>,

    /// Publishes click events to the messaging backbone (Kafka, or a no-op fallback).
    pub click_publisher: Arc<dyn ClickPublisher>,

    /// Concrete Kafka handle, present only when Kafka is configured. Used for health probes.
    pub kafka: Option<Arc<KafkaClickPublisher>>,
    /// Concrete ClickHouse handle, present only when ClickHouse is configured. Used for health probes.
    pub clickhouse: Option<Arc<ReconnectingClickHouse>>,

    /// Whether the dashboard auth cookie should be marked `Secure`.
    pub cookie_secure: bool,

    /// Prometheus exposition handle, rendered by `GET /metrics`.
    pub metrics_handle: PrometheusHandle,
}

impl AppState {
    /// Creates application state from pre-built repositories and infrastructure.
    ///
    /// Repositories are constructed once in `server.rs` and shared with the click
    /// consumer to avoid redundant allocations.
    ///
    /// # Arguments
    ///
    /// - `link_repo` / `token_repo` / `domain_repo` - pre-built repositories
    /// - `click_publisher` - publishes click events (Kafka or a no-op fallback)
    /// - `stats_reader` - reads click analytics (ClickHouse or an unavailable fallback)
    /// - `kafka` / `clickhouse` - concrete handles for health probes (when configured)
    /// - `cache` - cache implementation ([`RedisCache`](crate::infrastructure::cache::RedisCache) or [`NullCache`](crate::infrastructure::cache::NullCache))
    /// - `token_signing_secret` - HMAC key for token hashing; must match `TOKEN_SIGNING_SECRET`
    /// - `cookie_secure` - whether the dashboard session cookie is marked `Secure`
    /// - `block_private_urls` - whether to reject shortening private/loopback/local URLs
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        link_repo: Arc<PgLinkRepository>,
        token_repo: Arc<PgTokenRepository>,
        domain_repo: Arc<PgDomainRepository>,
        click_publisher: Arc<dyn ClickPublisher>,
        stats_reader: Arc<dyn ClickStatsReader>,
        kafka: Option<Arc<KafkaClickPublisher>>,
        clickhouse: Option<Arc<ReconnectingClickHouse>>,
        cache: Arc<dyn CacheService>,
        token_signing_secret: String,
        cookie_secure: bool,
        block_private_urls: bool,
    ) -> Self {
        let link_service = Arc::new(LinkService::new(
            link_repo.clone(),
            domain_repo.clone(),
            block_private_urls,
        ));
        let stats_service = Arc::new(StatsService::new(stats_reader, link_repo));
        let auth_service = Arc::new(AuthService::new(token_repo, token_signing_secret));
        let domain_service = Arc::new(DomainService::new(domain_repo));

        let metrics_handle = crate::observability::metrics::install_prometheus_recorder();

        Self {
            link_service,
            stats_service,
            auth_service,
            domain_service,
            cache,
            click_publisher,
            kafka,
            clickhouse,
            cookie_secure,
            metrics_handle,
        }
    }

    /// Health probe for Kafka (non-critical): `true` only when configured and reachable.
    pub async fn kafka_health(&self) -> bool {
        match &self.kafka {
            Some(k) => k.health_check(),
            None => false,
        }
    }

    /// Health probe for ClickHouse (non-critical): `true` only when configured and reachable.
    pub async fn clickhouse_health(&self) -> bool {
        match &self.clickhouse {
            Some(ch) => ch.health_check().await,
            None => false,
        }
    }
}
