use crate::domain::entities::{Domain, NewDomain, UpdateDomain};
use crate::domain::repositories::DomainRepository;
use crate::error::AppError;
use serde_json::json;
use std::sync::Arc;

pub struct DomainService<R: DomainRepository> {
    repository: Arc<R>,
}

impl<R: DomainRepository> DomainService<R> {
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }

    /// Создать новый домен
    pub async fn create_domain(
        &self,
        domain: String,
        is_default: bool,
        description: Option<String>,
    ) -> Result<Domain, AppError> {
        // Валидация домена
        self.validate_domain_name(&domain)?;

        // Проверка существования
        if self.repository.find_by_name(&domain).await?.is_some() {
            return Err(AppError::conflict(
                "Domain already exists",
                json!({"domain": domain}),
            ));
        }

        let new_domain = NewDomain {
            domain,
            is_default,
            description,
        };

        let created = self.repository.create(new_domain).await?;

        // Если это default, снять флаг с остальных
        if is_default {
            self.repository.set_default(created.id).await?;
        }

        Ok(created)
    }

    /// Список доменов
    pub async fn list_domains(&self, only_active: bool) -> Result<Vec<Domain>, AppError> {
        self.repository.list(only_active).await
    }

    /// Получить домен по имени
    pub async fn get_domain(&self, domain: &str) -> Result<Domain, AppError> {
        self.repository
            .find_by_name(domain)
            .await?
            .ok_or_else(|| AppError::not_found("Domain not found", json!({"domain": domain})))
    }

    /// Получить default домен
    pub async fn get_default_domain(&self) -> Result<Domain, AppError> {
        self.repository.get_default().await
    }

    /// Установить default домен
    pub async fn set_default(&self, domain_id: i64) -> Result<(), AppError> {
        self.repository.set_default(domain_id).await
    }

    /// Обновить домен
    pub async fn update_domain(
        &self,
        domain_id: i64,
        update: UpdateDomain,
    ) -> Result<Domain, AppError> {
        self.repository.update(domain_id, update).await
    }

    /// Удалить домен
    pub async fn delete_domain(&self, domain_id: i64) -> Result<(), AppError> {
        let domain = self
            .repository
            .find_by_id(domain_id)
            .await?
            .ok_or_else(|| AppError::not_found("Domain not found", json!({"id": domain_id})))?;

        // Нельзя удалить default домен
        if domain.is_default {
            return Err(AppError::bad_request(
                "Cannot delete default domain",
                json!({"hint": "Set another domain as default first"}),
            ));
        }

        // Проверить наличие ссылок
        let links_count = self.repository.count_links(domain_id).await?;
        if links_count > 0 {
            return Err(AppError::bad_request(
                "Cannot delete domain with existing links",
                json!({"links_count": links_count}),
            ));
        }

        self.repository.delete(domain_id).await
    }

    /// Валидация имени домена
    fn validate_domain_name(&self, domain: &str) -> Result<(), AppError> {
        if domain.is_empty() || domain.len() > 255 {
            return Err(AppError::bad_request(
                "Invalid domain name length",
                json!({"min": 1, "max": 255}),
            ));
        }

        // Простая проверка формата
        if !domain.contains('.') {
            return Err(AppError::bad_request(
                "Invalid domain format",
                json!({"hint": "Domain must contain at least one dot"}),
            ));
        }

        // Проверка на недопустимые символы
        if !domain
            .chars()
            .all(|c| c.is_alphanumeric() || c == '.' || c == '-')
        {
            return Err(AppError::bad_request(
                "Invalid characters in domain name",
                json!({"allowed": "a-z, 0-9, dots, hyphens"}),
            ));
        }

        Ok(())
    }
}
