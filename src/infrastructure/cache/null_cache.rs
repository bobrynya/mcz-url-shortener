use super::service::{CacheResult, CacheService};
use async_trait::async_trait;
use tracing::debug;

/// Пустая реализация кэша (когда CACHE_ENABLED=false)
pub struct NullCache;

impl NullCache {
    pub fn new() -> Self {
        debug!("Using NullCache (caching disabled)");
        Self
    }
}

impl Default for NullCache {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CacheService for NullCache {
    async fn get_url(&self, _short_code: &str) -> CacheResult<Option<String>> {
        Ok(None)
    }

    async fn set_url(
        &self,
        _short_code: &str,
        _original_url: &str,
        _ttl: Option<usize>,
    ) -> CacheResult<()> {
        Ok(())
    }

    async fn invalidate(&self, _short_code: &str) -> CacheResult<()> {
        Ok(())
    }

    async fn health_check(&self) -> bool {
        true
    }
}
