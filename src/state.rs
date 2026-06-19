//! Application state shared across HTTP handlers.
//!
//! Contains service instances, database pool, cache, and channels for
//! asynchronous click processing. Cloned for each request via Axum's
//! state extraction.

use std::sync::Arc;
use tokio::sync::mpsc;

use crate::application::services::{AuthService, DomainService, LinkService, StatsService};
use crate::domain::click_event::ClickEvent;
use crate::infrastructure::cache::CacheService;
use crate::infrastructure::persistence::{
    PgDomainRepository, PgLinkRepository, PgStatsRepository, PgTokenRepository,
};

/// Shared application state injected into HTTP handlers.
///
/// Contains all services and infrastructure components needed to process requests.
/// Cheap to clone due to `Arc` wrapping.
#[derive(Clone)]
pub struct AppState {
    pub link_service: Arc<LinkService<PgLinkRepository, PgDomainRepository>>,
    pub stats_service: Arc<StatsService<PgStatsRepository>>,
    pub auth_service: Arc<AuthService<PgTokenRepository>>,
    pub domain_service: Arc<DomainService<PgDomainRepository>>,

    pub cache: Arc<dyn CacheService>,

    pub click_sender: mpsc::Sender<ClickEvent>,

    /// Whether the dashboard auth cookie should be marked `Secure`.
    pub cookie_secure: bool,
}

impl AppState {
    /// Creates application state from pre-built repositories.
    ///
    /// Repositories are constructed once in `server.rs` and shared between the click
    /// worker and the application state to avoid redundant allocations.
    ///
    /// # Arguments
    ///
    /// - `link_repo` / `stats_repo` / `token_repo` / `domain_repo` - pre-built repositories
    /// - `click_sender` - channel sender for asynchronous click event processing
    /// - `cache` - cache implementation ([`RedisCache`](crate::infrastructure::cache::RedisCache) or [`NullCache`](crate::infrastructure::cache::NullCache))
    /// - `token_signing_secret` - HMAC key for token hashing; must match `TOKEN_SIGNING_SECRET`
    /// - `cookie_secure` - whether the dashboard session cookie is marked `Secure`
    /// - `block_private_urls` - whether to reject shortening private/loopback/local URLs
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        link_repo: Arc<PgLinkRepository>,
        stats_repo: Arc<PgStatsRepository>,
        token_repo: Arc<PgTokenRepository>,
        domain_repo: Arc<PgDomainRepository>,
        click_sender: mpsc::Sender<ClickEvent>,
        cache: Arc<dyn CacheService>,
        token_signing_secret: String,
        cookie_secure: bool,
        block_private_urls: bool,
    ) -> Self {
        let link_service = Arc::new(LinkService::new(
            link_repo,
            domain_repo.clone(),
            block_private_urls,
        ));
        let stats_service = Arc::new(StatsService::new(stats_repo));
        let auth_service = Arc::new(AuthService::new(token_repo, token_signing_secret));
        let domain_service = Arc::new(DomainService::new(domain_repo));

        Self {
            link_service,
            stats_service,
            auth_service,
            domain_service,
            cache,
            click_sender,
            cookie_secure,
        }
    }
}
