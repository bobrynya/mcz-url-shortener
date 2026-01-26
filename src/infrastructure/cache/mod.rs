//! Модуль кэширования для редиректов URL-shortener

mod null_cache;
mod redis_cache;
mod service;

pub use null_cache::NullCache;
pub use redis_cache::RedisCache;
pub use service::{CacheError, CacheResult, CacheService};
