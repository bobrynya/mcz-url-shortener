use axum::{Json, extract::State, http::StatusCode};

use crate::api::dto::health::{CheckStatus, HealthChecks, HealthResponse};
use crate::state::AppState;

/// GET /health - Health check endpoint
pub async fn health_handler(
    State(state): State<AppState>,
) -> Result<Json<HealthResponse>, (StatusCode, Json<HealthResponse>)> {
    // Проверяем БД
    let db_check = check_database(&state).await;

    // Проверяем очередь кликов
    let queue_check = check_click_queue(&state);

    // Проверяем Redis
    let cache_check = check_cache(&state).await;

    // Общий статус
    let all_healthy =
        db_check.status == "ok" && queue_check.status == "ok" && cache_check.status == "ok";

    let response = HealthResponse {
        status: if all_healthy { "healthy" } else { "degraded" }.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        checks: HealthChecks {
            database: db_check,
            click_queue: queue_check,
            cache: cache_check,
        },
    };

    if all_healthy {
        Ok(Json(response))
    } else {
        Err((StatusCode::SERVICE_UNAVAILABLE, Json(response)))
    }
}

/// Проверка подключения к БД
async fn check_database(state: &AppState) -> CheckStatus {
    match state.domain_service.get_default_domain().await {
        Ok(domain) => CheckStatus {
            status: "ok".to_string(),
            message: Some(format!("Connected, default domain: {}", domain.domain)),
        },
        Err(e) => CheckStatus {
            status: "error".to_string(),
            message: Some(format!("Database error: {}", e)),
        },
    }
}

/// Проверка очереди кликов
fn check_click_queue(state: &AppState) -> CheckStatus {
    // Проверяем, можем ли мы отправить в канал (не закрыт ли он)
    if state.click_sender.is_closed() {
        CheckStatus {
            status: "error".to_string(),
            message: Some("Click queue is closed".to_string()),
        }
    } else {
        CheckStatus {
            status: "ok".to_string(),
            message: Some(format!("Capacity: {}", state.click_sender.capacity())),
        }
    }
}

/// Проверка Redis кэша
async fn check_cache(state: &AppState) -> CheckStatus {
    if state.cache.health_check().await {
        CheckStatus {
            status: "ok".to_string(),
            message: Some("Redis connected".to_string()),
        }
    } else {
        CheckStatus {
            status: "error".to_string(),
            message: Some("Redis connection failed".to_string()),
        }
    }
}
