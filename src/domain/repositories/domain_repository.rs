use crate::domain::entities::{Domain, NewDomain, UpdateDomain};
use crate::error::AppError;
use async_trait::async_trait;

#[async_trait]
pub trait DomainRepository: Send + Sync {
    /// Создать новый домен
    async fn create(&self, new_domain: NewDomain) -> Result<Domain, AppError>;

    /// Найти домен по ID
    async fn find_by_id(&self, id: i64) -> Result<Option<Domain>, AppError>;

    /// Найти домен по имени
    async fn find_by_name(&self, domain: &str) -> Result<Option<Domain>, AppError>;

    /// Получить default домен
    async fn get_default(&self) -> Result<Domain, AppError>;

    /// Список всех доменов
    async fn list(&self, only_active: bool) -> Result<Vec<Domain>, AppError>;

    /// Обновить домен
    async fn update(&self, id: i64, update: UpdateDomain) -> Result<Domain, AppError>;

    /// Удалить домен (только если нет привязанных ссылок)
    async fn delete(&self, id: i64) -> Result<(), AppError>;

    /// Установить default домен (снимает флаг с остальных)
    async fn set_default(&self, id: i64) -> Result<(), AppError>;

    /// Количество ссылок на домене
    async fn count_links(&self, domain_id: i64) -> Result<i64, AppError>;
}
