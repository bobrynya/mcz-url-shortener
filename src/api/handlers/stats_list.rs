use axum::{
    Json,
    extract::{Query, State},
};

use crate::api::dto::pagination::StatsQueryParams;
use crate::api::dto::stats_list::{LinkStatsItem, PaginationMeta, StatsListResponse};
use crate::domain::repositories::StatsFilter;
use crate::error::AppError;
use crate::state::AppState;
use serde_json::json;

/// GET /stats - Получение списка всех ссылок со статистикой
pub async fn stats_list_handler(
    State(state): State<AppState>,
    Query(params): Query<StatsQueryParams>,
) -> Result<Json<StatsListResponse>, AppError> {
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

    // Получаем данные и общее количество
    let (all_stats, total_items) = tokio::try_join!(
        state.stats_service.get_all_stats(filter),
        state.stats_service.count_all_links()
    )?;

    // Формируем ответ
    let items = all_stats
        .into_iter()
        .map(|stat| LinkStatsItem {
            code: stat.code,
            long_url: stat.long_url,
            total_clicks: stat.total_clicks,
            created_at: stat.created_at,
        })
        .collect();

    let total_pages = ((total_items as f64) / (params.pagination.page_size as f64)).ceil() as u32;

    Ok(Json(StatsListResponse {
        items,
        pagination: PaginationMeta {
            page: params.pagination.page,
            page_size: params.pagination.page_size,
            total_items,
            total_pages,
        },
    }))
}
