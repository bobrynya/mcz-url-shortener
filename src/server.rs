use crate::config::Config;
use crate::domain::click_worker::run_click_worker;
use crate::infrastructure::cache::{CacheService, NullCache, RedisCache};
use crate::infrastructure::persistence::{PgDomainRepository, PgLinkRepository, PgStatsRepository};
use crate::routes::app_router;
use crate::state::AppState;

use anyhow::Result;
use axum::ServiceExt;
use axum::extract::Request;
use sqlx::PgPool;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Запуск HTTP сервера с полной инициализацией
pub async fn run(config: Config) -> Result<()> {
    // 1. Подключение к БД
    let pool = PgPool::connect(&config.database_url).await?;
    tracing::info!("Connected to database");

    // 2. Подключение к Redis (если включен)
    let cache: Arc<dyn CacheService> = if let Some(redis_url) = &config.redis_url {
        match RedisCache::connect(redis_url).await {
            Ok(redis) => {
                tracing::info!("Cache enabled (Redis)");
                Arc::new(redis)
            }
            Err(e) => {
                tracing::warn!("Failed to connect to Redis: {}. Using NullCache.", e);
                Arc::new(NullCache::new())
            }
        }
    } else {
        tracing::info!("Cache disabled (NullCache)");
        Arc::new(NullCache::new())
    };

    // 3. Создание очереди кликов
    let (click_tx, click_rx) = mpsc::channel(config.click_queue_capacity);

    // 4. Запуск воркера
    let pool_arc = Arc::new(pool.clone());
    let stats_repository = Arc::new(PgStatsRepository::new(pool_arc.clone()));
    let domain_repository = Arc::new(PgDomainRepository::new(pool_arc.clone()));
    let link_repository = Arc::new(PgLinkRepository::new(pool_arc.clone()));
    tokio::spawn(run_click_worker(
        click_rx,
        stats_repository,
        domain_repository,
        link_repository,
    ));
    tracing::info!("Click worker started");

    // 5. Создание состояния приложения (добавляем cache)
    let state = AppState::new(Arc::new(pool), click_tx, cache);

    // 6. Создание роутера
    let app = app_router(state);

    // 7. Запуск HTTP сервера
    let addr: SocketAddr = config.listen_addr.parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("Listening on http://{addr}");

    axum::serve(
        listener,
        ServiceExt::<Request>::into_make_service_with_connect_info::<SocketAddr>(app),
    )
    .await?;

    Ok(())
}
