//! Binary entry point for the URL shortener service.
//!
//! Initializes logging, loads configuration, and starts the HTTP server.

use anyhow::Result;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};
use url_shortener::{config, server};

#[tokio::main]
async fn main() -> Result<()> {
    if let Err(e) = dotenvy::dotenv() {
        eprintln!("Failed to load .env: {} (using system environment)", e);
    }

    let env_filter = EnvFilter::try_from_default_env().or_else(|_| EnvFilter::try_new("info"))?;

    let cfg = config::load_from_env()?;

    match cfg.log_format.as_str() {
        "json" => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(
                    tracing_subscriber::fmt::layer()
                        .json()
                        .with_current_span(true)
                        .with_span_list(false),
                )
                .init();
        }
        _ => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(tracing_subscriber::fmt::layer())
                .init();
        }
    }

    tracing::info!(
        listen = %cfg.listen_addr,
        log_level = %cfg.log_level,
        click_batch_size = %cfg.click_batch_size,
        "Configuration loaded"
    );

    tracing::info!("Starting url-shortener");

    server::run(cfg).await
}
