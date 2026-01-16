use axum::{
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use std::net::SocketAddr;

use crate::domain::click_event::ClickEvent;
use crate::error::AppError;
use crate::state::AppState;

/// GET /:code - Редирект на оригинальный URL
pub async fn redirect_handler(
    State(state): State<AppState>,
    Path(code): Path<String>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Result<Response, AppError> {
    // Получаем ссылку через сервис
    let link = state.link_service.get_link_by_code(&code).await?;

    // Извлекаем метаданные для статистики
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let referer = headers
        .get(header::REFERER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Извлекаем IP-адрес
    let ip = Some(addr.ip().to_string());

    // Отправляем событие клика в очередь (неблокирующее)
    let click_event = ClickEvent {
        link_id: link.id,
        user_agent,
        referer,
        ip,
    };

    // Игнорируем ошибку отправки (не критично)
    let _ = state.click_sender.try_send(click_event);

    // Выполняем редирект
    Ok((StatusCode::FOUND, [(header::LOCATION, link.long_url)]).into_response())
}
