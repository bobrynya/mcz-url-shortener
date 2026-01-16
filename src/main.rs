mod api;
mod application;
mod domain;
mod error;
mod infrastructure;
mod routes;
mod state;
mod utils;

use crate::infrastructure::persistence::PgStatsRepository;
use crate::{routes::app_router, state::AppState};
use domain::click_event::ClickEvent;
use domain::click_worker::run_click_worker;
use sqlx::PgPool;
use std::sync::Arc;
use std::{env, net::SocketAddr};
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    let database_url = env::var("DATABASE_URL")?;
    let base_url = env::var("BASE_URL").unwrap_or_else(|_| "https://s.test.com".to_string());
    let listen = env::var("LISTEN").unwrap_or_else(|_| "0.0.0.0:3000".to_string());

    // Подключаемся к базе данных
    let pool = PgPool::connect(&database_url).await?;
    tracing::info!("Connected to database");

    // Создаём очередь для кликов
    let (click_tx, click_rx) = mpsc::channel::<ClickEvent>(10_000);

    // Создаём репозиторий для воркера
    let stats_repository = Arc::new(PgStatsRepository::new(Arc::new(pool.clone())));

    // Запускаем воркер для обработки кликов
    tokio::spawn(run_click_worker(click_rx, stats_repository));
    tracing::info!("Click worker started");

    // Создаём состояние приложения с сервисами
    let state = AppState::new(Arc::new(pool), base_url, click_tx);

    // Запускаем HTTP-сервер
    let addr: SocketAddr = listen.parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("Listening on http://{addr}");

    let app = app_router(state);

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}
