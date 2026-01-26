use async_trait::async_trait;
use std::fmt;

/// Ошибки кэширования
#[derive(Debug)]
pub enum CacheError {
    ConnectionError(String),
    OperationError(String),
}

impl fmt::Display for CacheError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::ConnectionError(e) => write!(f, "Cache connection error: {}", e),
            Self::OperationError(e) => write!(f, "Cache operation error: {}", e),
        }
    }
}

impl std::error::Error for CacheError {}

pub type CacheResult<T> = Result<T, CacheError>;

/// Трейт для абстракции кэширования редиректов
#[async_trait]
pub trait CacheService: Send + Sync {
    /// Получить URL по короткому коду
    async fn get_url(&self, short_code: &str) -> CacheResult<Option<String>>;

    /// Сохранить mapping short_code -> original_url
    async fn set_url(
        &self,
        short_code: &str,
        original_url: &str,
        ttl_seconds: Option<usize>,
    ) -> CacheResult<()>;

    /// Удалить из кэша (инвалидация)
    async fn invalidate(&self, short_code: &str) -> CacheResult<()>;

    /// Проверка работоспособности
    async fn health_check(&self) -> bool;
}
