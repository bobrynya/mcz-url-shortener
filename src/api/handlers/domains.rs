use crate::api::dto::domain::{DomainItem, DomainListResponse};
use crate::error::AppError;
use crate::state::AppState;
use axum::{Json, extract::State};

/// GET /domains - Получение списка всех ссылок со статистикой
pub async fn domain_list_handler(
    State(state): State<AppState>,
) -> Result<Json<DomainListResponse>, AppError> {
    let all_domains = state.domain_service.list_domains(false).await?;

    // Формируем ответ
    let items = all_domains
        .into_iter()
        .map(|domain| DomainItem {
            domain: domain.domain,
            is_default: domain.is_default,
            is_active: domain.is_active,
            description: domain.description,
            created_at: domain.created_at,
            updated_at: domain.updated_at,
        })
        .collect();

    Ok(Json(DomainListResponse { items }))
}
