use crate::api::routes::app_router;
use crate::config::Config;
use crate::domain::click_worker::run_click_worker;
use crate::infrastructure::persistence::PgStatsRepository;
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

    // 2. Создание очереди кликов
    let (click_tx, click_rx) = mpsc::channel(config.click_queue_capacity);

    // 3. Запуск воркера
    let stats_repository = Arc::new(PgStatsRepository::new(Arc::new(pool.clone())));
    tokio::spawn(run_click_worker(click_rx, stats_repository));
    tracing::info!("Click worker started");

    // 4. Создание состояния приложения (без base_url)
    let state = AppState::new(Arc::new(pool), click_tx);

    // 5. Создание роутера
    let app = app_router(state);

    // 6. Запуск HTTP сервера
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
