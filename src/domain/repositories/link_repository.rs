use crate::domain::entities::{Link, NewLink};
use crate::error::AppError;
use async_trait::async_trait;

/// Репозиторий для работы со ссылками
#[async_trait]
pub trait LinkRepository: Send + Sync {
    /// Создать новую ссылку
    async fn create(&self, new_link: NewLink) -> Result<Link, AppError>;

    /// Найти ссылку по короткому коду
    async fn find_by_code(&self, code: &str) -> Result<Option<Link>, AppError>;

    /// Найти ссылку по оригинальному URL
    async fn find_by_long_url(&self, long_url: &str) -> Result<Option<Link>, AppError>;
}
