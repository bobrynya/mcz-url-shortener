use std::sync::Arc;

use crate::domain::entities::{Link, NewLink};
use crate::domain::repositories::LinkRepository;
use crate::error::AppError;
use crate::utils::code_generator;
use crate::utils::url_normalizer::normalize_url;
use serde_json::json;

/// Сервис для работы со ссылками
pub struct LinkService<R: LinkRepository> {
    repository: Arc<R>,
    base_url: String,
}

impl<R: LinkRepository> LinkService<R> {
    pub fn new(repository: Arc<R>, base_url: String) -> Self {
        Self {
            repository,
            base_url,
        }
    }

    /// Создать короткую ссылку
    /// Если ссылка уже существует - вернуть существующую
    pub async fn create_short_link(&self, long_url: String) -> Result<Link, AppError> {
        // 1. Валидация и нормализация URL
        let normalized_url = normalize_url(&long_url).map_err(|e| {
            AppError::bad_request("Invalid URL format", json!({ "reason": e.to_string() }))
        })?;

        // 2. Проверяем, существует ли уже ссылка
        if let Some(existing_link) = self.repository.find_by_long_url(&normalized_url).await? {
            return Ok(existing_link);
        }

        // 3. Генерируем уникальный код
        let code = self.generate_unique_code().await?;

        // 4. Создаём новую ссылку
        let new_link = NewLink {
            code,
            long_url: normalized_url,
        };

        self.repository.create(new_link).await
    }

    /// Создать несколько коротких ссылок (пакетная операция)
    pub async fn create_short_links(&self, long_urls: Vec<String>) -> Result<Vec<Link>, AppError> {
        if long_urls.is_empty() {
            return Err(AppError::bad_request(
                "URLs list cannot be empty",
                json!({}),
            ));
        }

        if long_urls.len() > 100 {
            return Err(AppError::bad_request(
                "Too many URLs in batch",
                json!({ "max": 100, "provided": long_urls.len() }),
            ));
        }

        let mut results = Vec::with_capacity(long_urls.len());

        for url in long_urls {
            let link = self.create_short_link(url).await?;
            results.push(link);
        }

        Ok(results)
    }

    /// Получить полную информацию о ссылке по коду
    pub async fn get_link_by_code(&self, code: &str) -> Result<Link, AppError> {
        self.repository
            .find_by_code(code)
            .await?
            .ok_or_else(|| AppError::not_found("Short link not found", json!({ "code": code })))
    }

    /// Получить короткий URL (base_url + code)
    pub fn get_short_url(&self, code: &str) -> String {
        format!("{}/{}", self.base_url.trim_end_matches('/'), code)
    }

    /// Генерация уникального кода (с повторными попытками при коллизии)
    async fn generate_unique_code(&self) -> Result<String, AppError> {
        const MAX_ATTEMPTS: usize = 10;

        for _ in 0..MAX_ATTEMPTS {
            let code = code_generator::generate_code();

            // Проверяем, не занят ли код
            if self.repository.find_by_code(&code).await?.is_none() {
                return Ok(code);
            }
        }

        Err(AppError::internal(
            "Failed to generate unique code",
            json!({ "reason": "Too many collisions" }),
        ))
    }
}
