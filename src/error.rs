use axum::{
    Json,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::Error as SqlxError;

#[derive(Serialize)]
struct ErrorBody {
    error: ErrorInfo,
}

#[derive(Serialize)]
struct ErrorInfo {
    code: &'static str,
    message: String,
    details: Value,
}

#[derive(Debug)]
pub enum AppError {
    Validation { message: String, details: Value },
    NotFound { message: String, details: Value },
    Conflict { message: String, details: Value },
    Unauthorized { message: String, details: Value },
    Internal { message: String, details: Value },
}

impl AppError {
    pub fn bad_request(message: impl Into<String>, details: Value) -> Self {
        Self::Validation {
            message: message.into(),
            details,
        }
    }

    pub fn not_found(message: impl Into<String>, details: Value) -> Self {
        Self::NotFound {
            message: message.into(),
            details,
        }
    }

    pub fn conflict(message: impl Into<String>, details: Value) -> Self {
        Self::Conflict {
            message: message.into(),
            details,
        }
    }

    pub fn internal(message: impl Into<String>, details: Value) -> Self {
        Self::Internal {
            message: message.into(),
            details,
        }
    }

    pub fn unauthorized(message: impl Into<String>, details: Value) -> Self {
        Self::Unauthorized {
            message: message.into(),
            details,
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code, message, details, add_www_authenticate) = match self {
            AppError::Validation { message, details } => (
                StatusCode::BAD_REQUEST,
                "validation_error",
                message,
                details,
                false,
            ),
            AppError::NotFound { message, details } => {
                (StatusCode::NOT_FOUND, "not_found", message, details, false)
            }
            AppError::Conflict { message, details } => {
                (StatusCode::CONFLICT, "conflict", message, details, false)
            }
            AppError::Unauthorized { message, details } => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                message,
                details,
                true,
            ),
            AppError::Internal { message, details } => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                message,
                details,
                false,
            ),
        };

        let body = ErrorBody {
            error: ErrorInfo {
                code,
                message,
                details,
            },
        };

        if add_www_authenticate {
            let mut headers = HeaderMap::new();
            headers.insert(header::WWW_AUTHENTICATE, "Bearer".parse().unwrap());
            (status, headers, Json(body)).into_response()
        } else {
            (status, Json(body)).into_response()
        }
    }
}

/// Автоматическая конвертация sqlx::Error в AppError
impl From<SqlxError> for AppError {
    fn from(e: SqlxError) -> Self {
        map_sqlx_error(e)
    }
}

/// Маппинг sqlx::Error в AppError с детальной обработкой
pub fn map_sqlx_error(e: SqlxError) -> AppError {
    #[cfg(debug_assertions)]
    tracing::debug!(error = ?e, "Full sqlx error in debug mode");

    match &e {
        SqlxError::Database(db_err) => {
            // Нарушение уникальности
            if db_err.is_unique_violation() {
                metrics::counter!("database_errors_total", "type" => "unique_violation")
                    .increment(1);

                let constraint = db_err.constraint().unwrap_or("unknown");
                let (message, field) = match constraint {
                    "links_code_key" => ("This short code is already in use", "code"),
                    "links_long_url_key" => ("This URL has already been shortened", "long_url"),
                    "api_tokens_token_hash_key" => ("Token already exists", "token"),
                    _ => {
                        tracing::warn!(
                            constraint = constraint,
                            "Unknown unique constraint violated"
                        );
                        ("Resource already exists", constraint)
                    }
                };

                return AppError::conflict(
                    message,
                    json!({
                        "field": field,
                        "constraint": constraint,
                        "type": "unique_violation"
                    }),
                );
            }

            // Нарушение внешнего ключа
            if db_err.is_foreign_key_violation() {
                metrics::counter!("database_errors_total", "type" => "foreign_key_violation")
                    .increment(1);

                let constraint = db_err.constraint().unwrap_or("unknown");
                let message = match constraint {
                    "link_clicks_link_id_fkey" => "The referenced link does not exist",
                    _ => {
                        tracing::warn!(
                            constraint = constraint,
                            "Unknown foreign key constraint violated"
                        );
                        "Referenced resource not found"
                    }
                };

                return AppError::bad_request(
                    message,
                    json!({
                        "constraint": constraint,
                        "type": "foreign_key_violation"
                    }),
                );
            }

            // Нарушение check-ограничения
            if db_err.is_check_violation() {
                metrics::counter!("database_errors_total", "type" => "check_violation")
                    .increment(1);

                let constraint = db_err.constraint().unwrap_or("unknown");
                tracing::warn!(constraint = constraint, "Check constraint violated");

                return AppError::bad_request(
                    "Data validation failed",
                    json!({
                        "constraint": constraint,
                        "type": "check_violation"
                    }),
                );
            }

            // Остальные ошибки БД
            tracing::error!(
                code = ?db_err.code(),
                message = ?db_err.message(),
                constraint = ?db_err.constraint(),
                "Unhandled database error"
            );
            metrics::counter!("database_errors_total", "type" => "other").increment(1);

            AppError::internal(
                "Database constraint violation",
                json!({ "code": db_err.code() }),
            )
        }

        // ... остальной код без изменений
        SqlxError::RowNotFound => {
            metrics::counter!("database_errors_total", "type" => "row_not_found").increment(1);
            AppError::not_found("Record not found", json!({}))
        }

        SqlxError::PoolTimedOut => {
            tracing::warn!("Database connection pool timed out");
            metrics::counter!("database_errors_total", "type" => "pool_timeout").increment(1);
            AppError::internal(
                "Service temporarily unavailable",
                json!({ "retryable": true, "type": "pool_timeout" }),
            )
        }

        SqlxError::PoolClosed => {
            tracing::error!("Database connection pool is closed");
            metrics::counter!("database_errors_total", "type" => "pool_closed").increment(1);
            AppError::internal(
                "Service unavailable",
                json!({ "retryable": false, "type": "pool_closed" }),
            )
        }

        SqlxError::Io(_) => {
            tracing::warn!(error = ?e, "Database I/O error");
            metrics::counter!("database_errors_total", "type" => "io_error").increment(1);
            AppError::internal(
                "Database connection issue",
                json!({ "retryable": true, "type": "io_error" }),
            )
        }

        SqlxError::Protocol(_) => {
            tracing::error!(error = ?e, "Database protocol error");
            metrics::counter!("database_errors_total", "type" => "protocol_error").increment(1);
            AppError::internal(
                "Database protocol error",
                json!({ "retryable": false, "type": "protocol_error" }),
            )
        }

        _ => {
            tracing::error!(error = ?e, "Unexpected database error");
            metrics::counter!("database_errors_total", "type" => "unknown").increment(1);
            AppError::internal("Database operation failed", json!({}))
        }
    }
}
