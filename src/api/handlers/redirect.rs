use axum::{
    extract::{Path, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Redirect},
};
use std::net::SocketAddr;
use tracing::{debug, error};

use crate::domain::click_event::ClickEvent;
use crate::error::AppError;
use crate::state::AppState;
use crate::utils::extract_domain::extract_domain_from_headers;

/// GET /:code - Редирект на оригинальный URL
pub async fn redirect_handler(
    Path(code): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<SocketAddr>,
) -> Result<impl IntoResponse, AppError> {
    // 1. Получаем домен из Host header
    let domain = extract_domain_from_headers(&headers)?;

    // 2. Строим cache key: domain:code
    let cache_key = format!("{}:{}", domain, code);

    // 3. Пытаемся получить из кэша
    let long_url = match state.cache.get_url(&cache_key).await {
        Ok(Some(cached_url)) => {
            debug!("Cache HIT for {}", cache_key);
            cached_url
        }
        Ok(None) => {
            debug!("Cache MISS for {}", cache_key);

            // 4. Находим домен в БД
            let domain_entity = state.domain_service.get_domain(&domain).await?;

            // 5. Ищем ссылку по коду и domain_id
            let link = state
                .link_service
                .get_link_by_code(&code, domain_entity.id)
                .await?;

            // 6. Сохраняем в кэш (асинхронно)
            let cache_clone = state.cache.clone();
            let cache_key_clone = cache_key.clone();
            let url_clone = link.long_url.clone();
            tokio::spawn(async move {
                if let Err(e) = cache_clone
                    .set_url(&cache_key_clone, &url_clone, None)
                    .await
                {
                    error!("Failed to cache URL: {}", e);
                }
            });

            link.long_url
        }
        Err(e) => {
            error!("Cache error: {}", e);

            // Fallback на БД
            let domain_entity = state.domain_service.get_domain(&domain).await?;
            let link = state
                .link_service
                .get_link_by_code(&code, domain_entity.id)
                .await?;

            link.long_url
        }
    };

    // 7. Отправляем событие клика в очередь (воркер сам найдёт link_id)
    let click_event = ClickEvent::new(
        domain,
        code,
        Some(addr.ip().to_string()),
        headers
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok()),
        headers.get(header::REFERER).and_then(|v| v.to_str().ok()),
    );

    // Игнорируем ошибку, если очередь переполнена
    let _ = state.click_sender.try_send(click_event);

    // 8. Редирект
    Ok(Redirect::temporary(&long_url))
}
