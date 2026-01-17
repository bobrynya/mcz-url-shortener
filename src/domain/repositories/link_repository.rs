use crate::domain::entities::{Link, NewLink};
use crate::error::AppError;
use async_trait::async_trait;

#[async_trait]
pub trait LinkRepository: Send + Sync {
    /// Создать новую ссылку
    async fn create(&self, new_link: NewLink) -> Result<Link, AppError>;

    /// Найти ссылку по коду и домену
    async fn find_by_code(&self, code: &str, domain_id: i64) -> Result<Option<Link>, AppError>;

    /// Найти ссылку по длинному URL и домену
    async fn find_by_long_url(
        &self,
        long_url: &str,
        domain_id: i64,
    ) -> Result<Option<Link>, AppError>;

    /// Список ссылок с пагинацией и фильтром по домену
    async fn list(
        &self,
        page: i64,
        page_size: i64,
        domain_id: Option<i64>, // None = все домены
    ) -> Result<Vec<Link>, AppError>;

    /// Количество ссылок
    async fn count(&self, domain_id: Option<i64>) -> Result<i64, AppError>;
}
