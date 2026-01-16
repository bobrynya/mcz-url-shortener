use axum::{
    Json,
    extract::{Path, Query, State},
};

use crate::api::dto::pagination::StatsQueryParams;
use crate::api::dto::stats::StatsResponse;
use crate::api::dto::stats_list::PaginationMeta;
use crate::domain::repositories::StatsFilter;
use crate::error::AppError;
use crate::state::AppState;
use serde_json::json;

/// GET /stats/:code - Получение статистики по коду
pub async fn stats_handler(
    State(state): State<AppState>,
    Path(code): Path<String>,
    Query(params): Query<StatsQueryParams>,
) -> Result<Json<StatsResponse>, AppError> {
    // Валидация параметров пагинации
    let (offset, limit) = params
        .pagination
        .validate_and_get_offset_limit()
        .map_err(|e| AppError::bad_request(e, json!({})))?;

    // Создаём фильтр
    let filter = StatsFilter {
        from_date: params.date_filter.from,
        to_date: params.date_filter.to,
        offset,
        limit,
    };

    // Получаем детальную статистику
    let detailed_stats = state
        .stats_service
        .get_detailed_stats(&code, filter)
        .await?;

    // Вычисляем количество страниц
    let total_pages =
        ((detailed_stats.total_clicks as f64) / (params.pagination.page_size as f64)).ceil() as u32;

    // Преобразуем в DTO
    let response = StatsResponse {
        code: detailed_stats.link.code,
        long_url: detailed_stats.link.long_url,
        created_at: detailed_stats.link.created_at,
        total_clicks: detailed_stats.total_clicks,
        recent_clicks: detailed_stats
            .recent_clicks
            .into_iter()
            .map(|click| crate::api::dto::clicks::ClickInfo {
                clicked_at: click.clicked_at,
                user_agent: click.user_agent,
                referer: click.referer,
                ip: click.ip,
            })
            .collect(),
        pagination: PaginationMeta {
            page: params.pagination.page,
            page_size: params.pagination.page_size,
            total_items: detailed_stats.total_clicks,
            total_pages,
        },
    };

    Ok(Json(response))
}
