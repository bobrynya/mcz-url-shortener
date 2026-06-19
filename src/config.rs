//! Application configuration loaded from environment variables.
//!
//! Configuration is loaded once at startup and validated before the server starts.
//!
//! ## Configuration Methods
//!
//! ### Method 1: Full URLs (simpler for local development)
//!
//! ```bash
//! export DATABASE_URL="postgres://user:pass@localhost:5432/dbname"
//! export REDIS_URL="redis://localhost:6379/0"
//! ```
//!
//! ### Method 2: Individual components (recommended for production)
//!
//! ```bash
//! export DB_HOST="localhost"
//! export DB_PORT="5432"
//! export DB_USER="postgres"
//! export DB_PASSWORD="password"
//! export DB_NAME="url-shortener"
//!
//! export REDIS_HOST="localhost"
//! export REDIS_PORT="6379"
//! export REDIS_PASSWORD=""
//! export REDIS_DB="0"
//! ```
//!
//! If `DATABASE_URL` is not set, it will be automatically constructed from
//! `DB_HOST`, `DB_PORT`, `DB_USER`, `DB_PASSWORD`, and `DB_NAME`.
//!
//! ## Required Variables
//!
//! Either `DATABASE_URL` or all of (`DB_HOST`, `DB_USER`, `DB_PASSWORD`, `DB_NAME`)
//!
//! ## Optional Variables
//!
//! - `REDIS_URL` / `REDIS_HOST` - Redis connection (enables caching if set)
//! - `LISTEN` - Bind address (default: `0.0.0.0:3000`)
//! - `RUST_LOG` - Log level (default: `info`)
//! - `LOG_FORMAT` - Log format: `text` or `json` (default: `text`)
//! - `CLICK_QUEUE_CAPACITY` - Click event buffer size (default: 10000, min: 100)

use anyhow::{Context, Result};
use std::env;

/// Service configuration loaded from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub redis_url: Option<String>,
    pub listen_addr: String,
    pub log_level: String,
    pub log_format: String,
    pub click_queue_capacity: usize,
    /// When true, rate limiting reads client IP from X-Forwarded-For / X-Real-IP headers.
    /// Enable only when the service is behind a trusted reverse proxy.
    pub behind_proxy: bool,
    /// Default TTL (seconds) for cached URL mappings in Redis.
    /// Has no effect when Redis is not configured.
    pub cache_ttl_seconds: u64,
    /// Maximum number of click events processed concurrently by the background worker.
    pub click_worker_concurrency: usize,
    /// HMAC signing secret used to hash API tokens before storage.
    /// Loaded from `TOKEN_SIGNING_SECRET`. Must be non-empty.
    pub token_signing_secret: String,
    /// When true, the dashboard auth cookie is marked `Secure` (sent over HTTPS only).
    /// Loaded from `COOKIE_SECURE` (default: `true`). Set to `false` for local HTTP development.
    pub cookie_secure: bool,
    /// When true, refuse to shorten URLs pointing at private/loopback/link-local
    /// hosts (or `localhost`). Loaded from `BLOCK_PRIVATE_URLS` (default: `true`).
    pub block_private_urls: bool,

    // ── PgPool settings ─────────────────────────────────────────────────────
    /// Maximum number of connections in the pool (`DB_MAX_CONNECTIONS`, default: 10).
    pub db_max_connections: u32,
    /// Timeout for acquiring a connection from the pool in seconds
    /// (`DB_CONNECT_TIMEOUT`, default: 30).
    pub db_connect_timeout: u64,
    /// Idle connection lifetime in seconds before it is closed
    /// (`DB_IDLE_TIMEOUT`, default: 600).
    pub db_idle_timeout: u64,
    /// Maximum connection lifetime in seconds (`DB_MAX_LIFETIME`, default: 1800).
    pub db_max_lifetime: u64,
}

impl Config {
    /// Loads configuration from environment variables.
    ///
    /// # Errors
    ///
    /// Returns an error if required database configuration is missing.
    pub fn from_env() -> Result<Self> {
        // Load database URL with priority
        let database_url =
            Self::load_database_url().context("Failed to load database configuration")?;

        // Load Redis URL (optional)
        let redis_url = Self::load_redis_url();

        // Load other configuration
        let listen_addr = env::var("LISTEN").unwrap_or_else(|_| "0.0.0.0:3000".to_string());
        let log_level = env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
        let log_format = env::var("LOG_FORMAT").unwrap_or_else(|_| "text".to_string());

        let click_queue_capacity = env::var("CLICK_QUEUE_CAPACITY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10_000);

        let behind_proxy = env::var("BEHIND_PROXY")
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
            .unwrap_or(false);

        let cache_ttl_seconds = env::var("CACHE_TTL_SECONDS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3600);

        let click_worker_concurrency = env::var("CLICK_WORKER_CONCURRENCY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(4);

        let token_signing_secret =
            env::var("TOKEN_SIGNING_SECRET").context("TOKEN_SIGNING_SECRET must be set")?;

        let cookie_secure = env::var("COOKIE_SECURE")
            .map(|v| !(v.eq_ignore_ascii_case("false") || v == "0"))
            .unwrap_or(true);

        let block_private_urls = env::var("BLOCK_PRIVATE_URLS")
            .map(|v| !(v.eq_ignore_ascii_case("false") || v == "0"))
            .unwrap_or(true);

        let db_max_connections = env::var("DB_MAX_CONNECTIONS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);

        let db_connect_timeout = env::var("DB_CONNECT_TIMEOUT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);

        let db_idle_timeout = env::var("DB_IDLE_TIMEOUT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(600);

        let db_max_lifetime = env::var("DB_MAX_LIFETIME")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1800);

        Ok(Self {
            database_url,
            redis_url,
            listen_addr,
            log_level,
            log_format,
            click_queue_capacity,
            behind_proxy,
            cache_ttl_seconds,
            click_worker_concurrency,
            token_signing_secret,
            cookie_secure,
            block_private_urls,
            db_max_connections,
            db_connect_timeout,
            db_idle_timeout,
            db_max_lifetime,
        })
    }

    /// Loads database URL with fallback to component-based configuration.
    ///
    /// Priority:
    /// 1. `DATABASE_URL` environment variable
    /// 2. Constructed from `DB_HOST`, `DB_PORT`, `DB_USER`, `DB_PASSWORD`, `DB_NAME`
    fn load_database_url() -> Result<String> {
        // Priority 1: Use DATABASE_URL if provided
        if let Ok(url) = env::var("DATABASE_URL") {
            return Ok(url);
        }

        // Priority 2: Build from components
        let host = env::var("DB_HOST").unwrap_or_else(|_| "localhost".to_string());
        let port = env::var("DB_PORT").unwrap_or_else(|_| "5432".to_string());
        let user =
            env::var("DB_USER").context("DB_USER must be set when DATABASE_URL is not provided")?;
        let password = env::var("DB_PASSWORD")
            .context("DB_PASSWORD must be set when DATABASE_URL is not provided")?;
        let name =
            env::var("DB_NAME").context("DB_NAME must be set when DATABASE_URL is not provided")?;

        Ok(format!(
            "postgres://{}:{}@{}:{}/{}",
            user, password, host, port, name
        ))
    }

    /// Loads Redis URL with fallback to component-based configuration.
    ///
    /// Priority:
    /// 1. `REDIS_URL` environment variable
    /// 2. Constructed from `REDIS_HOST`, `REDIS_PORT`, `REDIS_PASSWORD`, `REDIS_DB`
    ///
    /// Returns `None` if Redis is not configured.
    fn load_redis_url() -> Option<String> {
        // Priority 1: Use REDIS_URL if provided
        if let Ok(url) = env::var("REDIS_URL") {
            return Some(url);
        }

        // Priority 2: Build from components (if REDIS_HOST is set)
        let host = env::var("REDIS_HOST").ok()?;
        let port = env::var("REDIS_PORT").unwrap_or_else(|_| "6379".to_string());
        let password = env::var("REDIS_PASSWORD").ok();
        let db = env::var("REDIS_DB").unwrap_or_else(|_| "0".to_string());

        let url = if let Some(pwd) = password {
            // Empty password means no authentication
            if pwd.is_empty() {
                format!("redis://{}:{}/{}", host, port, db)
            } else {
                format!("redis://:{}@{}:{}/{}", pwd, host, port, db)
            }
        } else {
            format!("redis://{}:{}/{}", host, port, db)
        };

        Some(url)
    }

    /// Validates the configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `click_queue_capacity` is less than 100
    /// - `log_format` is not `text` or `json`
    /// - `listen_addr` is invalid
    pub fn validate(&self) -> Result<()> {
        // Validate queue capacity
        if self.click_queue_capacity < 100 {
            anyhow::bail!(
                "CLICK_QUEUE_CAPACITY must be at least 100, got {}",
                self.click_queue_capacity
            );
        }

        if self.click_queue_capacity > 1_000_000 {
            anyhow::bail!(
                "CLICK_QUEUE_CAPACITY is too large (max: 1000000), got {}",
                self.click_queue_capacity
            );
        }

        // Validate log format
        if self.log_format != "text" && self.log_format != "json" {
            anyhow::bail!(
                "LOG_FORMAT must be 'text' or 'json', got '{}'",
                self.log_format
            );
        }

        // Validate listen address format
        if !self.listen_addr.contains(':') {
            anyhow::bail!(
                "LISTEN must be in format 'host:port', got '{}'",
                self.listen_addr
            );
        }

        // Validate database URL format
        if !self.database_url.starts_with("postgres://")
            && !self.database_url.starts_with("postgresql://")
        {
            anyhow::bail!(
                "DATABASE_URL must start with 'postgres://' or 'postgresql://', got '{}'",
                self.database_url
            );
        }

        // Validate Redis URL format (if present)
        if let Some(ref redis_url) = self.redis_url
            && !redis_url.starts_with("redis://")
            && !redis_url.starts_with("rediss://")
        {
            anyhow::bail!(
                "REDIS_URL must start with 'redis://' or 'rediss://', got '{}'",
                redis_url
            );
        }

        // Validate cache TTL
        if self.cache_ttl_seconds == 0 {
            anyhow::bail!("CACHE_TTL_SECONDS must be greater than 0");
        }

        // Validate click worker concurrency
        if self.click_worker_concurrency == 0 || self.click_worker_concurrency > 256 {
            anyhow::bail!(
                "CLICK_WORKER_CONCURRENCY must be between 1 and 256, got {}",
                self.click_worker_concurrency
            );
        }

        // Validate token signing secret
        if self.token_signing_secret.is_empty() {
            anyhow::bail!("TOKEN_SIGNING_SECRET must not be empty");
        }

        // Validate pool settings
        if self.db_max_connections == 0 {
            anyhow::bail!("DB_MAX_CONNECTIONS must be at least 1");
        }
        if self.db_connect_timeout == 0 {
            anyhow::bail!("DB_CONNECT_TIMEOUT must be greater than 0");
        }

        Ok(())
    }

    /// Returns whether Redis caching is enabled.
    pub fn is_cache_enabled(&self) -> bool {
        self.redis_url.is_some()
    }

    /// Prints configuration summary (without sensitive data).
    pub fn print_summary(&self) {
        tracing::info!("Configuration loaded:");
        tracing::info!("  Listen address: {}", self.listen_addr);
        tracing::info!("  Database: {}", mask_connection_string(&self.database_url));

        if let Some(ref redis_url) = self.redis_url {
            tracing::info!("  Redis: {} (enabled)", mask_connection_string(redis_url));
        } else {
            tracing::info!("  Redis: disabled");
        }

        tracing::info!("  Log level: {}", self.log_level);
        tracing::info!("  Log format: {}", self.log_format);
        tracing::info!("  Click queue capacity: {}", self.click_queue_capacity);
    }
}

/// Masks sensitive information in connection strings for logging.
///
/// Replaces password with `***` in URLs like:
/// - `postgres://user:password@host:port/db` → `postgres://user:***@host:port/db`
/// - `redis://:password@host:port/db` → `redis://:***@host:port/db`
fn mask_connection_string(url: &str) -> String {
    if let Some(start) = url.find("://") {
        let scheme_end = start + 3;
        let rest = &url[scheme_end..];

        if let Some(at_pos) = rest.find('@') {
            let credentials = &rest[..at_pos];
            let host_part = &rest[at_pos..];

            // Check if there's a password (contains ':')
            if let Some(colon_pos) = credentials.rfind(':') {
                let username = &credentials[..colon_pos];
                return format!("{}://{}:***{}", &url[..start], username, host_part);
            }
        }
    }

    url.to_string()
}

/// Loads and validates configuration from environment variables.
///
/// # Errors
///
/// Returns an error if required variables are missing or validation fails.
///
/// # Note
///
/// This function expects environment variables to be already loaded
/// (e.g., via `dotenvy::dotenv()` in `main.rs`).
pub fn load_from_env() -> Result<Config> {
    let config = Config::from_env()?;
    config.validate()?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    fn test_mask_connection_string() {
        assert_eq!(
            mask_connection_string("postgres://user:secret123@localhost:5432/db"),
            "postgres://user:***@localhost:5432/db"
        );

        assert_eq!(
            mask_connection_string("redis://:password@localhost:6379/0"),
            "redis://:***@localhost:6379/0"
        );

        assert_eq!(
            mask_connection_string("postgres://localhost:5432/db"),
            "postgres://localhost:5432/db"
        );
    }

    #[test]
    fn test_config_validation() {
        let mut config = Config {
            database_url: "postgres://localhost/test".to_string(),
            redis_url: None,
            listen_addr: "0.0.0.0:3000".to_string(),
            log_level: "info".to_string(),
            log_format: "text".to_string(),
            click_queue_capacity: 10_000,
            behind_proxy: false,
            cache_ttl_seconds: 3600,
            click_worker_concurrency: 4,
            token_signing_secret: "test-secret".to_string(),
            cookie_secure: true,
            block_private_urls: true,
            db_max_connections: 10,
            db_connect_timeout: 30,
            db_idle_timeout: 600,
            db_max_lifetime: 1800,
        };

        assert!(config.validate().is_ok());

        // Test invalid queue capacity
        config.click_queue_capacity = 50;
        assert!(config.validate().is_err());

        config.click_queue_capacity = 10_000;

        // Test invalid log format
        config.log_format = "invalid".to_string();
        assert!(config.validate().is_err());

        config.log_format = "json".to_string();
        assert!(config.validate().is_ok());

        // Test invalid listen address
        config.listen_addr = "3000".to_string();
        assert!(config.validate().is_err());

        config.listen_addr = "0.0.0.0:3000".to_string();

        // Test invalid database URL
        config.database_url = "mysql://localhost/test".to_string();
        assert!(config.validate().is_err());
    }

    #[test]
    #[serial]
    fn test_load_database_url_from_components() {
        // SAFETY: Tests are run serially due to #[serial], so no concurrent access
        unsafe {
            // Ensure the higher-priority var is absent so component-building is exercised
            // deterministically even when DATABASE_URL is set in the ambient environment.
            env::remove_var("DATABASE_URL");
            env::set_var("DB_HOST", "testhost");
            env::set_var("DB_PORT", "5433");
            env::set_var("DB_USER", "testuser");
            env::set_var("DB_PASSWORD", "testpass");
            env::set_var("DB_NAME", "testdb");
        }

        let url = Config::load_database_url().unwrap();

        assert_eq!(url, "postgres://testuser:testpass@testhost:5433/testdb");

        // Cleanup
        unsafe {
            env::remove_var("DB_HOST");
            env::remove_var("DB_PORT");
            env::remove_var("DB_USER");
            env::remove_var("DB_PASSWORD");
            env::remove_var("DB_NAME");
        }
    }

    #[test]
    #[serial]
    fn test_load_redis_url_from_components() {
        // SAFETY: Tests are run serially due to #[serial], so no concurrent access
        unsafe {
            // Ensure the higher-priority var is absent so component-building is exercised
            // deterministically even when REDIS_URL is set in the ambient environment.
            env::remove_var("REDIS_URL");
            env::set_var("REDIS_HOST", "redis-host");
            env::set_var("REDIS_PORT", "6380");
            env::set_var("REDIS_DB", "1");
        }

        let url = Config::load_redis_url().unwrap();
        assert_eq!(url, "redis://redis-host:6380/1");

        // Test with password
        unsafe {
            env::set_var("REDIS_PASSWORD", "secret");
        }
        let url = Config::load_redis_url().unwrap();
        assert_eq!(url, "redis://:secret@redis-host:6380/1");

        // Test with empty password (should be treated as no password)
        unsafe {
            env::set_var("REDIS_PASSWORD", "");
        }
        let url = Config::load_redis_url().unwrap();
        assert_eq!(url, "redis://redis-host:6380/1");

        // Cleanup
        unsafe {
            env::remove_var("REDIS_HOST");
            env::remove_var("REDIS_PORT");
            env::remove_var("REDIS_DB");
            env::remove_var("REDIS_PASSWORD");
        }
    }

    #[test]
    #[serial]
    fn test_database_url_priority() {
        // SAFETY: Tests are run serially
        unsafe {
            env::set_var("DATABASE_URL", "postgres://from-url:pass@host:5432/db");
            env::set_var("DB_USER", "from-components");
        }

        let url = Config::load_database_url().unwrap();

        // DATABASE_URL should take priority
        assert!(url.contains("from-url"));
        assert!(!url.contains("from-components"));

        // Cleanup
        unsafe {
            env::remove_var("DATABASE_URL");
            env::remove_var("DB_USER");
        }
    }

    #[test]
    #[serial]
    fn test_redis_url_priority() {
        // SAFETY: Tests are run serially
        unsafe {
            env::set_var("REDIS_URL", "redis://from-url:6379/0");
            env::set_var("REDIS_HOST", "from-components");
        }

        let url = Config::load_redis_url().unwrap();

        // REDIS_URL should take priority
        assert!(url.contains("from-url"));
        assert!(!url.contains("from-components"));

        // Cleanup
        unsafe {
            env::remove_var("REDIS_URL");
            env::remove_var("REDIS_HOST");
        }
    }

    fn base_config() -> Config {
        Config {
            database_url: "postgres://localhost/test".to_string(),
            redis_url: None,
            listen_addr: "0.0.0.0:3000".to_string(),
            log_level: "info".to_string(),
            log_format: "text".to_string(),
            click_queue_capacity: 10_000,
            behind_proxy: false,
            cache_ttl_seconds: 3600,
            click_worker_concurrency: 4,
            token_signing_secret: "test-secret".to_string(),
            cookie_secure: true,
            block_private_urls: true,
            db_max_connections: 10,
            db_connect_timeout: 30,
            db_idle_timeout: 600,
            db_max_lifetime: 1800,
        }
    }

    #[test]
    fn test_validate_click_queue_capacity_too_large() {
        let mut c = base_config();
        c.click_queue_capacity = 1_000_001;
        assert!(c.validate().is_err());
    }

    #[test]
    fn test_validate_invalid_redis_url_format() {
        let mut c = base_config();
        c.redis_url = Some("mysql://localhost/0".to_string());
        assert!(c.validate().is_err());
    }

    #[test]
    fn test_validate_valid_redis_url_formats() {
        let mut c = base_config();
        c.redis_url = Some("redis://localhost:6379/0".to_string());
        assert!(c.validate().is_ok());

        c.redis_url = Some("rediss://localhost:6379/0".to_string());
        assert!(c.validate().is_ok());
    }

    #[test]
    fn test_validate_cache_ttl_zero() {
        let mut c = base_config();
        c.cache_ttl_seconds = 0;
        assert!(c.validate().is_err());
    }

    #[test]
    fn test_validate_click_worker_concurrency_bounds() {
        let mut c = base_config();
        c.click_worker_concurrency = 0;
        assert!(c.validate().is_err());

        c.click_worker_concurrency = 257;
        assert!(c.validate().is_err());

        c.click_worker_concurrency = 1;
        assert!(c.validate().is_ok());

        c.click_worker_concurrency = 256;
        assert!(c.validate().is_ok());
    }

    #[test]
    fn test_validate_empty_token_signing_secret() {
        let mut c = base_config();
        c.token_signing_secret = "".to_string();
        assert!(c.validate().is_err());
    }

    #[test]
    fn test_validate_db_max_connections_zero() {
        let mut c = base_config();
        c.db_max_connections = 0;
        assert!(c.validate().is_err());
    }

    #[test]
    fn test_validate_db_connect_timeout_zero() {
        let mut c = base_config();
        c.db_connect_timeout = 0;
        assert!(c.validate().is_err());
    }

    #[test]
    fn test_is_cache_enabled() {
        let mut c = base_config();
        assert!(!c.is_cache_enabled());

        c.redis_url = Some("redis://localhost:6379/0".to_string());
        assert!(c.is_cache_enabled());
    }
}
