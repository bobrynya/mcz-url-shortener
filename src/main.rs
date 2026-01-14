mod domain;
mod dto;
mod error;
mod handlers;
mod middleware;
mod routes;
mod state;
mod utils;

use crate::domain::{click_event::ClickEvent, click_worker::run_click_worker};
use crate::{routes::app_router, state::AppState};
use sqlx::PgPool;
use std::{env, net::SocketAddr};
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    let database_url = env::var("DATABASE_URL")?;
    let base_url = env::var("BASE_URL").unwrap_or_else(|_| "https://s.test.com/".to_string());
    let listen = env::var("LISTEN").unwrap_or_else(|_| "0.0.0.0:3000".to_string());

    let db = PgPool::connect(&database_url).await?;

    let (click_tx, click_rx) = mpsc::channel::<ClickEvent>(10_000);
    tokio::spawn(run_click_worker(click_rx, db.clone()));

    let state = AppState {
        db,
        base_url,
        click_tx,
    };

    let addr: SocketAddr = listen.parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::info!("listening on http://{addr}");

    let app = app_router(state);

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}
