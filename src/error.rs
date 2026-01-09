use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use serde_json::{Value, json};

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
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code, message, details) = match self {
            AppError::Validation { message, details } => (
                StatusCode::BAD_REQUEST,
                "validation_error",
                message,
                details,
            ),
            AppError::NotFound { message, details } => {
                (StatusCode::NOT_FOUND, "not_found", message, details)
            }
            AppError::Conflict { message, details } => {
                (StatusCode::CONFLICT, "conflict", message, details)
            }
            AppError::Internal { message, details } => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                message,
                details,
            ),
        };

        let body = ErrorBody {
            error: ErrorInfo {
                code,
                message,
                details,
            },
        };

        (status, Json(body)).into_response()
    }
}

pub fn map_sqlx_error(e: sqlx::Error) -> AppError {
    if let Some(db) = e.as_database_error()
        && db.is_unique_violation()
    {
        return AppError::conflict(
            "Unique constraint violation",
            json!({ "constraint": db.constraint() }),
        );
    }

    AppError::internal("Database error", json!({}))
}
