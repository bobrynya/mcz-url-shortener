use axum::{Json, extract::State};

use crate::api::dto::shorten::{ShortenRequest, ShortenResponse, ShortenedLinkItem};
use crate::error::AppError;
use crate::state::AppState;

/// POST /shorten - Создание короткой ссылки (единичной или пакетно)
pub async fn shorten_handler(
    State(state): State<AppState>,
    Json(payload): Json<ShortenRequest>,
) -> Result<Json<ShortenResponse>, AppError> {
    // Преобразуем запрос в список URL
    let urls = payload.into_urls();

    // Создаём ссылки через сервис (пакетно)
    let links = state.link_service.create_short_links(urls).await?;

    // Формируем ответ
    let items = links
        .into_iter()
        .map(|link| {
            let short_url = state.link_service.get_short_url(&link.code);
            ShortenedLinkItem {
                long_url: link.long_url,
                code: link.code,
                short_url,
            }
        })
        .collect();

    Ok(Json(ShortenResponse { items }))
}
