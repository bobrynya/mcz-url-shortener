use axum::{
    extract::{Path, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Redirect},
};
use std::net::SocketAddr;

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

    // 2. Находим домен в БД
    let domain_entity = state.domain_service.get_domain(&domain).await?;

    // 3. Ищем ссылку по коду и domain_id
    let link = state
        .link_service
        .get_link_by_code(&code, domain_entity.id)
        .await?;

    // 4. Отправляем событие клика в очередь
    let click_event = ClickEvent::new(
        link.id,
        Some(addr.ip().to_string()),
        headers
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok()),
        headers.get(header::REFERER).and_then(|v| v.to_str().ok()),
    );

    // Игнорируем ошибку, если очередь переполнена
    let _ = state.click_sender.try_send(click_event);

    // 5. Редирект
    Ok(Redirect::permanent(&link.long_url))
}
