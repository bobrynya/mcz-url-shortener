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

    let domain_id = if let Some(domain_name) = &params.domain {
        let domain = state.domain_service.get_domain(domain_name).await?;
        Some(domain.id)
    } else {
        None
    };

    let filter = StatsFilter::new(offset, limit)
        .with_domain(domain_id)
        .with_date_range(params.date_filter.from, params.date_filter.to);

    // Получаем детальную статистику
    let detailed_stats = state
        .stats_service
        .get_detailed_stats(&code, filter)
        .await?;

    // Вычисляем количество страниц
    let total_pages =
        ((detailed_stats.total as f64) / (params.pagination.page_size as f64)).ceil() as u32;

    // Преобразуем в DTO
    let response = StatsResponse {
        pagination: PaginationMeta {
            page: params.pagination.page,
            page_size: params.pagination.page_size,
            total_items: detailed_stats.total,
            total_pages,
        },
        code: detailed_stats.link.code,
        domain: detailed_stats.link.domain,
        long_url: detailed_stats.link.long_url,
        created_at: detailed_stats.link.created_at,
        total: detailed_stats.total,
        items: detailed_stats
            .items
            .into_iter()
            .map(|click| crate::api::dto::clicks::ClickInfo {
                clicked_at: click.clicked_at,
                user_agent: click.user_agent,
                referer: click.referer,
                ip: click.ip,
            })
            .collect(),
    };

    Ok(Json(response))
}
