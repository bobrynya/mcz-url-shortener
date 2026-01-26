use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::application::services::{AuthService, DomainService, LinkService, StatsService};
use crate::domain::click_event::ClickEvent;
use crate::infrastructure::cache::CacheService;
use crate::infrastructure::persistence::{
    PgDomainRepository, PgLinkRepository, PgStatsRepository, PgTokenRepository,
};

#[derive(Clone)]
pub struct AppState {
    // Сервисы
    pub link_service: Arc<LinkService<PgLinkRepository, PgDomainRepository>>,
    pub stats_service: Arc<StatsService<PgStatsRepository>>,
    pub auth_service: Arc<AuthService<PgTokenRepository>>,
    pub domain_service: Arc<DomainService<PgDomainRepository>>,

    // Кэш
    pub cache: Arc<dyn CacheService>,

    // Очередь для кликов
    pub click_sender: mpsc::Sender<ClickEvent>,
}

impl AppState {
    pub fn new(
        pool: Arc<PgPool>,
        click_sender: mpsc::Sender<ClickEvent>,
        cache: Arc<dyn CacheService>,
    ) -> Self {
        // Создаём репозитории
        let link_repo = Arc::new(PgLinkRepository::new(pool.clone()));
        let stats_repo = Arc::new(PgStatsRepository::new(pool.clone()));
        let token_repo = Arc::new(PgTokenRepository::new(pool.clone()));
        let domain_repo = Arc::new(PgDomainRepository::new(pool.clone()));

        // Создаём сервисы
        let link_service = Arc::new(LinkService::new(link_repo, domain_repo.clone()));
        let stats_service = Arc::new(StatsService::new(stats_repo));
        let auth_service = Arc::new(AuthService::new(token_repo));
        let domain_service = Arc::new(DomainService::new(domain_repo));

        Self {
            link_service,
            stats_service,
            auth_service,
            domain_service,
            cache,
            click_sender,
        }
    }
}
