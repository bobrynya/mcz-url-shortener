use axum::{Json, extract::State};
use validator::Validate;

use crate::api::dto::shorten::{
    BatchSummary, ShortenRequest, ShortenResponse, ShortenResultItem, UrlItem,
};
use crate::error::AppError;
use crate::state::AppState;

/// POST /shorten - Создание коротких ссылок
pub async fn shorten_handler(
    State(state): State<AppState>,
    Json(payload): Json<ShortenRequest>,
) -> Result<Json<ShortenResponse>, AppError> {
    payload.validate()?;

    let total = payload.urls.len();
    let mut results = Vec::with_capacity(total);
    let mut successful = 0;
    let mut failed = 0;

    for item in payload.urls {
        let long_url = item.url.clone();

        // Обрабатываем каждую ссылку независимо
        match process_single_url(&state, item).await {
            Ok((code, short_url)) => {
                successful += 1;
                results.push(ShortenResultItem::Success {
                    long_url,
                    code,
                    short_url,
                });
            }
            Err(err) => {
                failed += 1;
                results.push(ShortenResultItem::Error {
                    long_url,
                    error: err.to_error_info(),
                });
            }
        }
    }

    Ok(Json(ShortenResponse {
        summary: BatchSummary {
            total,
            successful,
            failed,
        },
        items: results,
    }))
}

async fn process_single_url(state: &AppState, item: UrlItem) -> Result<(String, String), AppError> {
    let domain = if let Some(domain_name) = item.domain {
        state.domain_service.get_domain(&domain_name).await?
    } else {
        state.domain_service.get_default_domain().await?
    };

    let link = state
        .link_service
        .create_short_link_for_domain(item.url, domain.id, item.custom_code)
        .await?;

    let short_url = state.link_service.get_short_url(&domain.domain, &link.code);

    Ok((link.code, short_url))
}
