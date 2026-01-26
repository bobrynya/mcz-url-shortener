use anyhow::Result;
use tracing_subscriber::EnvFilter;
use url_shortener::{config, server};

#[tokio::main]
async fn main() -> Result<()> {
    // Загрузка .env (игнорируем ошибку в production)
    if let Err(e) = dotenvy::dotenv() {
        eprintln!("Failed to load .env: {} (using system environment)", e);
    }

    // Инициализация логирования
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    tracing::info!("Starting url-shortener");

    // Загрузка конфигурации
    let cfg = config::load_from_env()?;

    tracing::info!(
        listen = %cfg.listen_addr,
        log_level = %cfg.log_level,
        click_queue_capacity = %cfg.click_queue_capacity,
        "Configuration loaded"
    );

    // Запуск сервера
    server::run(cfg).await
}
