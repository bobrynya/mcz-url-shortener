//! HTTP server initialization and runtime setup.
//!
//! Handles database connections, cache setup, worker spawning, and Axum server lifecycle.

use crate::config::Config;
use crate::domain::click_worker::run_click_worker;
use crate::infrastructure::cache::{CacheService, NullCache, RedisCache};
use crate::infrastructure::persistence::{
    PgDomainRepository, PgLinkRepository, PgStatsRepository, PgTokenRepository,
};
use crate::routes::app_router;
use crate::state::AppState;

use anyhow::Result;
use axum::ServiceExt;
use axum::extract::Request;
use sqlx::postgres::PgPoolOptions;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

/// Runs the HTTP server with the given configuration.
///
/// Initializes:
/// - PostgreSQL connection pool and runs pending migrations
/// - Redis cache (or [`NullCache`] fallback if Redis is unavailable or unconfigured)
/// - Shared repositories passed to both the click worker and [`AppState`]
/// - Background click worker for asynchronous click persistence
/// - Axum HTTP server with graceful shutdown on `SIGTERM` / `Ctrl-C`
///
/// # Shutdown
///
/// On shutdown signal the HTTP server stops accepting new connections and waits
/// for in-flight requests to complete. Afterwards the click worker drains the
/// remaining events from its channel before exiting.
///
/// # Errors
///
/// Returns an error if the database connection, migration, or server bind fails.
pub async fn run(config: Config) -> Result<()> {
    let pool = PgPoolOptions::new()
        .max_connections(config.db_max_connections)
        .acquire_timeout(Duration::from_secs(config.db_connect_timeout))
        .idle_timeout(Duration::from_secs(config.db_idle_timeout))
        .max_lifetime(Duration::from_secs(config.db_max_lifetime))
        .connect(&config.database_url)
        .await?;
    tracing::info!("Connected to database");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to migrate");

    let cache: Arc<dyn CacheService> = if let Some(redis_url) = &config.redis_url {
        match RedisCache::connect(redis_url, config.cache_ttl_seconds).await {
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

    let (click_tx, click_rx) = mpsc::channel(config.click_queue_capacity);

    // Repositories created once and shared between click worker and AppState.
    let pool_arc = Arc::new(pool);
    let link_repo = Arc::new(PgLinkRepository::new(pool_arc.clone()));
    let stats_repo = Arc::new(PgStatsRepository::new(pool_arc.clone()));
    let token_repo = Arc::new(PgTokenRepository::new(pool_arc.clone()));
    let domain_repo = Arc::new(PgDomainRepository::new(pool_arc.clone()));

    let worker_handle = tokio::spawn(run_click_worker(
        click_rx,
        stats_repo.clone(),
        domain_repo.clone(),
        link_repo.clone(),
        config.click_worker_concurrency,
    ));
    tracing::info!("Click worker started");

    let state = AppState::new(
        link_repo,
        stats_repo,
        token_repo,
        domain_repo,
        click_tx,
        cache,
        config.token_signing_secret.clone(),
        config.cookie_secure,
        config.block_private_urls,
    );

    let app = app_router(state, config.behind_proxy);

    let addr: SocketAddr = config.listen_addr.parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("Listening on http://{addr}");

    axum::serve(
        listener,
        ServiceExt::<Request>::into_make_service_with_connect_info::<SocketAddr>(app),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    // serve() has returned: AppState is dropped, click_tx inside it is dropped.
    // The worker's channel will drain and then close naturally.
    tracing::info!("HTTP server stopped, draining click queue...");
    worker_handle.await.ok();
    tracing::info!("Click worker stopped, shutdown complete");

    Ok(())
}

/// Resolves on Ctrl-C (all platforms) or SIGTERM (Unix).
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("Shutdown signal received");
}
