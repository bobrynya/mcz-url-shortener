//! HTTP server initialization and runtime setup.
//!
//! Handles database connections, cache setup, click ingestion wiring, and the
//! Axum server lifecycle.

use crate::config::Config;
use crate::domain::repositories::{ClickPublisher, ClickStatsReader};
use crate::infrastructure::cache::{CacheService, NullCache, RedisCache};
use crate::infrastructure::messaging::click_consumer::run_click_consumer;
use crate::infrastructure::messaging::{KafkaClickPublisher, NoopClickPublisher};
use crate::infrastructure::persistence::{
    ClickHouseConfig, ClickHouseStatsReader, ClickSink, PgDomainRepository, PgLinkRepository,
    PgTokenRepository, ReconnectingClickHouse, UnavailableStatsReader,
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
use tokio_util::sync::CancellationToken;

/// Runs the HTTP server with the given configuration.
///
/// Initializes:
/// - PostgreSQL connection pool and runs pending migrations
/// - Redis cache (or [`NullCache`] fallback if Redis is unavailable or unconfigured)
/// - Kafka click publisher and ClickHouse stats reader/sink (optional)
/// - Background Kafka→ClickHouse click consumer (when both are configured)
/// - Axum HTTP server with graceful shutdown on `SIGTERM` / `Ctrl-C`
///
/// # Shutdown
///
/// On shutdown signal the HTTP server stops accepting new connections and waits
/// for in-flight requests to complete. Afterwards the click consumer is cancelled
/// and joined.
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

    // Repositories created once and shared between the click consumer and AppState.
    let pool_arc = Arc::new(pool);
    let link_repo = Arc::new(PgLinkRepository::new(pool_arc.clone()));
    let token_repo = Arc::new(PgTokenRepository::new(pool_arc.clone()));
    let domain_repo = Arc::new(PgDomainRepository::new(pool_arc.clone()));

    // ClickHouse (read + sink) — optional.
    let clickhouse: Option<Arc<ReconnectingClickHouse>> =
        config.clickhouse_url.as_ref().map(|url| {
            Arc::new(ReconnectingClickHouse::new(ClickHouseConfig {
                url: url.clone(),
                database: config.clickhouse_database.clone(),
                user: config.clickhouse_user.clone(),
                password: config.clickhouse_password.clone(),
                retry_interval: Duration::from_secs(30),
            }))
        });
    let stats_reader: Arc<dyn ClickStatsReader> = match &clickhouse {
        Some(ch) => Arc::new(ClickHouseStatsReader::new(ch.clone())),
        None => Arc::new(UnavailableStatsReader),
    };

    // Kafka publisher — optional.
    let kafka: Option<Arc<KafkaClickPublisher>> = match &config.kafka_brokers {
        Some(brokers) => {
            match KafkaClickPublisher::new(brokers, config.kafka_clicks_topic.clone()) {
                Ok(p) => Some(Arc::new(p)),
                Err(e) => {
                    tracing::warn!(error = %e, "Kafka producer init failed; clicks will be dropped");
                    None
                }
            }
        }
        None => None,
    };
    let click_publisher: Arc<dyn ClickPublisher> = match &kafka {
        Some(p) => p.clone(),
        None => Arc::new(NoopClickPublisher),
    };

    // Spawn the consumer only when both Kafka and ClickHouse are configured.
    let shutdown_token = CancellationToken::new();
    let consumer_handle = match (&config.kafka_brokers, &clickhouse) {
        (Some(brokers), Some(ch)) => {
            let sink: Arc<dyn ClickSink> = ch.clone();
            let handle = tokio::spawn(run_click_consumer(
                brokers.clone(),
                config.kafka_consumer_group.clone(),
                config.kafka_clicks_topic.clone(),
                sink,
                config.click_batch_size,
                Duration::from_millis(config.click_batch_flush_ms),
                shutdown_token.clone(),
            ));
            tracing::info!("Click consumer started (Kafka → ClickHouse)");
            Some(handle)
        }
        _ => {
            tracing::warn!("Kafka and/or ClickHouse not configured; click ingestion disabled");
            None
        }
    };

    let state = AppState::new(
        link_repo,
        token_repo,
        domain_repo,
        click_publisher,
        stats_reader,
        kafka,
        clickhouse,
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

    // serve() has returned: stop the click consumer and wait for it to drain.
    tracing::info!("HTTP server stopped, shutting down click consumer...");
    shutdown_token.cancel();
    if let Some(handle) = consumer_handle {
        handle.await.ok();
    }
    tracing::info!("Click consumer stopped, shutdown complete");

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
