use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::application::services::{AuthService, LinkService, StatsService};
use crate::domain::click_event::ClickEvent;
use crate::infrastructure::persistence::{PgLinkRepository, PgStatsRepository, PgTokenRepository};

#[derive(Clone)]
pub struct AppState {
    // Сервисы
    pub link_service: Arc<LinkService<PgLinkRepository>>,
    pub stats_service: Arc<StatsService<PgStatsRepository>>,
    pub auth_service: Arc<AuthService<PgTokenRepository>>,

    // Очередь для кликов
    pub click_sender: mpsc::Sender<ClickEvent>,
}

impl AppState {
    pub fn new(
        pool: Arc<PgPool>,
        base_url: String,
        click_sender: mpsc::Sender<ClickEvent>,
    ) -> Self {
        // Создаём репозитории
        let link_repo = Arc::new(PgLinkRepository::new(pool.clone()));
        let stats_repo = Arc::new(PgStatsRepository::new(pool.clone()));
        let token_repo = Arc::new(PgTokenRepository::new(pool.clone()));

        // Создаём сервисы
        let link_service = Arc::new(LinkService::new(link_repo, base_url.clone()));
        let stats_service = Arc::new(StatsService::new(stats_repo));
        let auth_service = Arc::new(AuthService::new(token_repo));

        Self {
            link_service,
            stats_service,
            auth_service,
            click_sender,
        }
    }
}
