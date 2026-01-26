use anyhow::{Context, Result};
use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub redis_url: Option<String>,
    pub listen_addr: String,
    pub log_level: String,
    pub click_queue_capacity: usize,
}

impl Config {
    /// Загрузка из переменных окружения
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            database_url: env::var("DATABASE_URL").context("DATABASE_URL must be set")?,
            redis_url: env::var("REDIS_URL").ok(),
            listen_addr: env::var("LISTEN").unwrap_or_else(|_| "0.0.0.0:3000".to_string()),
            log_level: env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string()),
            click_queue_capacity: env::var("CLICK_QUEUE_CAPACITY")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10_000),
        })
    }

    /// Валидация конфигурации
    pub fn validate(&self) -> Result<()> {
        if self.click_queue_capacity < 100 {
            anyhow::bail!("CLICK_QUEUE_CAPACITY must be at least 100");
        }

        Ok(())
    }

    /// Проверка, включен ли кэш
    pub fn is_cache_enabled(&self) -> bool {
        self.redis_url.is_some()
    }
}

pub fn load_from_env() -> Result<Config> {
    let config = Config::from_env()?;
    config.validate()?;
    Ok(config)
}
