//! Handler for health check endpoint.

use axum::{Json, extract::State, http::StatusCode};

use crate::api::dto::health::{CheckStatus, HealthChecks, HealthResponse};
use crate::state::AppState;

/// Returns service health status with component checks.
///
/// # Endpoint
///
/// `GET /api/health`
///
/// # Response Codes
///
/// - **200 OK**: All components healthy
/// - **503 Service Unavailable**: One or more components degraded
///
/// # Components Checked
///
/// 1. **Database** (critical): Tests default domain query
/// 2. **Cache** (non-critical): Tests Redis PING
/// 3. **Kafka** (non-critical): Fetches topic metadata
/// 4. **ClickHouse** (non-critical): Tests connectivity
///
/// Only the database is critical: when it is down the endpoint returns 503.
/// Any other component being down yields a `degraded` status but still 200.
///
/// # Response
///
/// ```json
/// {
///   "status": "healthy",
///   "version": "0.1.0",
///   "checks": {
///     "database": { "status": "ok", "message": "Connected, default domain: s.example.com" },
///     "cache": { "status": "ok", "message": "Redis connected" },
///     "kafka": { "status": "ok", "message": "Reachable" },
///     "clickhouse": { "status": "ok", "message": "Reachable" }
///   }
/// }
/// ```
pub async fn health_handler(
    State(state): State<AppState>,
) -> Result<Json<HealthResponse>, (StatusCode, Json<HealthResponse>)> {
    let db_check = check_database(&state).await;
    let cache_check = check_cache(&state).await;
    let kafka_check = check_kafka(&state).await;
    let ch_check = check_clickhouse(&state).await;

    let critical_ok = db_check.status == "ok";
    let all_ok = critical_ok
        && cache_check.status == "ok"
        && kafka_check.status == "ok"
        && ch_check.status == "ok";

    let response = HealthResponse {
        status: if all_ok { "healthy" } else { "degraded" }.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        checks: HealthChecks {
            database: db_check,
            cache: cache_check,
            kafka: kafka_check,
            clickhouse: ch_check,
        },
    };

    if critical_ok {
        Ok(Json(response)) // 200 even when degraded
    } else {
        Err((StatusCode::SERVICE_UNAVAILABLE, Json(response)))
    }
}

/// Checks database connectivity by querying the default domain.
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

/// Checks Kafka connectivity by fetching topic metadata (non-critical).
async fn check_kafka(state: &AppState) -> CheckStatus {
    if state.kafka_health().await {
        CheckStatus {
            status: "ok".to_string(),
            message: Some("Reachable".to_string()),
        }
    } else {
        CheckStatus {
            status: "error".to_string(),
            message: Some("Kafka unavailable or not configured".to_string()),
        }
    }
}

/// Checks ClickHouse connectivity (non-critical).
async fn check_clickhouse(state: &AppState) -> CheckStatus {
    if state.clickhouse_health().await {
        CheckStatus {
            status: "ok".to_string(),
            message: Some("Reachable".to_string()),
        }
    } else {
        CheckStatus {
            status: "error".to_string(),
            message: Some("ClickHouse unavailable or not configured".to_string()),
        }
    }
}

/// Checks cache connectivity via PING command.
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
