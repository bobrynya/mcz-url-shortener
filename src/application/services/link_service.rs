use std::sync::Arc;

use crate::domain::entities::{Link, NewLink};
use crate::domain::repositories::{DomainRepository, LinkRepository};
use crate::error::AppError;
use crate::utils::code_generator::{generate_code, validate_custom_code};
use crate::utils::url_normalizer::normalize_url;
use serde_json::json;

/// Сервис для работы со ссылками
pub struct LinkService<L: LinkRepository, D: DomainRepository> {
    link_repository: Arc<L>,
    domain_repository: Arc<D>,
}

impl<L: LinkRepository, D: DomainRepository> LinkService<L, D> {
    pub fn new(link_repository: Arc<L>, domain_repository: Arc<D>) -> Self {
        Self {
            link_repository,
            domain_repository,
        }
    }

    /// Создать короткую ссылку (использует default домен)
    pub async fn create_short_link(
        &self,
        long_url: String,
        custom_code: Option<String>,
    ) -> Result<Link, AppError> {
        // Получаем default домен
        let default_domain = self.domain_repository.get_default().await?;
        self.create_short_link_for_domain(long_url, default_domain.id, custom_code)
            .await
    }

    /// Создать короткую ссылку для конкретного домена
    pub async fn create_short_link_for_domain(
        &self,
        long_url: String,
        domain_id: i64,
        custom_code: Option<String>,
    ) -> Result<Link, AppError> {
        // 1. Валидация и нормализация URL
        let normalized_url = normalize_url(&long_url).map_err(|e| {
            AppError::bad_request("Invalid URL format", json!({ "reason": e.to_string() }))
        })?;

        // 2. Проверяем, существует ли уже ссылка для этого домена
        if let Some(existing_link) = self
            .link_repository
            .find_by_long_url(&normalized_url, domain_id)
            .await?
        {
            return Ok(existing_link);
        }

        // 3. Генерируем или валидируем кастомный код
        let code = if let Some(custom) = custom_code {
            // Валидация кастомного кода
            validate_custom_code(&custom)?;

            // Проверка уникальности
            if self
                .link_repository
                .find_by_code(&custom, domain_id)
                .await?
                .is_some()
            {
                return Err(AppError::conflict(
                    "Custom code already exists for this domain",
                    json!({ "code": custom, "domain_id": domain_id }),
                ));
            }

            custom
        } else {
            // Генерируем случайный код
            self.generate_unique_code(domain_id).await?
        };

        // 4. Создаём новую ссылку
        let new_link = NewLink {
            code,
            long_url: normalized_url,
            domain_id,
        };

        self.link_repository.create(new_link).await
    }

    /// Получить полную информацию о ссылке по коду и домену
    pub async fn get_link_by_code(&self, code: &str, domain_id: i64) -> Result<Link, AppError> {
        self.link_repository
            .find_by_code(code, domain_id)
            .await?
            .ok_or_else(|| {
                AppError::not_found(
                    "Short link not found",
                    json!({ "code": code, "domain_id": domain_id }),
                )
            })
    }

    /// Получить короткий URL (domain + code)
    pub fn get_short_url(&self, domain: &str, code: &str) -> String {
        format!("https://{}/{}", domain.trim_end_matches('/'), code)
    }

    /// Генерация уникального кода в рамках домена
    async fn generate_unique_code(&self, domain_id: i64) -> Result<String, AppError> {
        const MAX_ATTEMPTS: usize = 10;

        for _ in 0..MAX_ATTEMPTS {
            let code = generate_code();

            // Проверяем, не занят ли код в рамках этого домена
            if self
                .link_repository
                .find_by_code(&code, domain_id)
                .await?
                .is_none()
            {
                return Ok(code);
            }
        }

        Err(AppError::internal(
            "Failed to generate unique code",
            json!({ "reason": "Too many collisions" }),
        ))
    }
}
