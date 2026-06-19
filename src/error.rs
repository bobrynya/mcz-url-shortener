//! Application error types and HTTP response conversion.
//!
//! Defines a unified error type ([`AppError`]) that maps to HTTP status codes
//! and provides structured JSON error responses for API consumers.
//!
//! ## Error Categories
//!
//! - [`AppError::Validation`] - Invalid input (400 Bad Request)
//! - [`AppError::NotFound`] - Resource not found (404 Not Found)
//! - [`AppError::Conflict`] - Duplicate resource (409 Conflict)
//! - [`AppError::Unauthorized`] - Authentication failed (401 Unauthorized)
//! - [`AppError::Internal`] - Server error (500 Internal Server Error)
//!
//! ## Database Error Mapping
//!
//! SQLx errors are automatically converted via [`From<SqlxError>`] with:
//! - Unique constraint violations → [`AppError::Conflict`]
//! - Foreign key violations → [`AppError::Validation`]
//! - Row not found → [`AppError::NotFound`]
//! - Connection pool issues → [`AppError::Internal`] with retry hints
//!
//! ## Observability
//!
//! All database errors emit metrics via `metrics::counter!` for monitoring.

use axum::{
    Json,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::Error as SqlxError;
use validator::ValidationErrors;

/// Internal structure for JSON error response body.
#[derive(Serialize)]
struct ErrorBody {
    error: ErrorInfo,
}

/// Structured error information returned in API responses.
#[derive(Debug, Serialize, Clone)]
pub struct ErrorInfo {
    pub code: &'static str,
    pub message: String,
    pub details: Value,
}

/// Application-level error type with context and HTTP mapping.
///
/// Each variant corresponds to an HTTP status code and includes both
/// a human-readable message and structured details for debugging.
#[derive(Debug)]
pub enum AppError {
    Validation { message: String, details: Value },
    NotFound { message: String, details: Value },
    Gone { message: String, details: Value },
    Conflict { message: String, details: Value },
    Unauthorized { message: String, details: Value },
    Internal { message: String, details: Value },
}

impl AppError {
    /// Creates a validation error (400 Bad Request).
    pub fn bad_request(message: impl Into<String>, details: Value) -> Self {
        Self::Validation {
            message: message.into(),
            details,
        }
    }

    /// Creates a not found error (404 Not Found).
    pub fn not_found(message: impl Into<String>, details: Value) -> Self {
        Self::NotFound {
            message: message.into(),
            details,
        }
    }

    /// Creates a conflict error (409 Conflict).
    pub fn conflict(message: impl Into<String>, details: Value) -> Self {
        Self::Conflict {
            message: message.into(),
            details,
        }
    }

    /// Creates a gone error (410 Gone) for resources that intentionally no longer exist.
    pub fn gone(message: impl Into<String>, details: Value) -> Self {
        Self::Gone {
            message: message.into(),
            details,
        }
    }

    /// Creates an internal server error (500 Internal Server Error).
    pub fn internal(message: impl Into<String>, details: Value) -> Self {
        Self::Internal {
            message: message.into(),
            details,
        }
    }

    /// Creates an unauthorized error (401 Unauthorized).
    pub fn unauthorized(message: impl Into<String>, details: Value) -> Self {
        Self::Unauthorized {
            message: message.into(),
            details,
        }
    }

    /// Converts the error into structured error info for serialization.
    pub fn to_error_info(self) -> ErrorInfo {
        let (code, message, details) = match self {
            AppError::Validation { message, details } => ("validation_error", message, details),
            AppError::NotFound { message, details } => ("not_found", message, details),
            AppError::Gone { message, details } => ("gone", message, details),
            AppError::Conflict { message, details } => ("conflict", message, details),
            AppError::Unauthorized { message, details } => ("unauthorized", message, details),
            AppError::Internal { message, details } => ("internal_error", message, details),
        };

        ErrorInfo {
            code,
            message,
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
            AppError::Gone { message, details } => {
                (StatusCode::GONE, "gone", message, details, false)
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

impl From<SqlxError> for AppError {
    fn from(e: SqlxError) -> Self {
        map_sqlx_error(e)
    }
}

/// Maps SQLx errors to application errors with detailed context.
///
/// Handles constraint violations, connection issues, and other database errors
/// with appropriate HTTP status codes and metrics emission.
pub fn map_sqlx_error(e: SqlxError) -> AppError {
    #[cfg(debug_assertions)]
    tracing::debug!(error = ?e, "Full sqlx error in debug mode");

    match &e {
        SqlxError::Database(db_err) => {
            if db_err.is_unique_violation() {
                metrics::counter!("database_errors_total", "type" => "unique_violation")
                    .increment(1);

                let constraint = db_err.constraint().unwrap_or("unknown");
                let (message, field) = match constraint {
                    "links_code_key" | "idx_links_unique_code_per_domain" => {
                        ("This short code is already in use", "code")
                    }
                    "links_long_url_key" | "idx_links_unique_long_url_per_domain" => {
                        ("This URL has already been shortened", "long_url")
                    }
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

impl std::error::Error for AppError {}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::Validation { message, .. } => write!(f, "Validation error: {}", message),
            AppError::NotFound { message, .. } => write!(f, "Not found: {}", message),
            AppError::Gone { message, .. } => write!(f, "Gone: {}", message),
            AppError::Conflict { message, .. } => write!(f, "Conflict: {}", message),
            AppError::Unauthorized { message, .. } => write!(f, "Unauthorized: {}", message),
            AppError::Internal { message, .. } => write!(f, "Internal error: {}", message),
        }
    }
}

impl From<ValidationErrors> for AppError {
    fn from(errors: ValidationErrors) -> Self {
        let details = json!({
            "fields": errors
                .field_errors()
                .iter()
                .map(|(field, errors)| {
                    (
                        field.to_string(),
                        errors
                            .iter()
                            .map(|e| {
                                json!({
                                    "code": e.code,
                                    "message": e.message.as_ref().map(|m| m.to_string()),
                                    "params": e.params
                                })
                            })
                            .collect::<Vec<_>>()
                    )
                })
                .collect::<std::collections::HashMap<_, _>>()
        });

        AppError::Validation {
            message: "Request validation failed".to_string(),
            details,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    fn status(err: AppError) -> StatusCode {
        err.into_response().status()
    }

    // ── IntoResponse status codes ─────────────────────────────────────────────

    #[test]
    fn test_validation_error_is_400() {
        assert_eq!(
            status(AppError::bad_request("bad input", json!({}))),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn test_not_found_is_404() {
        assert_eq!(
            status(AppError::not_found("missing", json!({}))),
            StatusCode::NOT_FOUND
        );
    }

    #[test]
    fn test_gone_is_410() {
        assert_eq!(
            status(AppError::gone("deleted", json!({}))),
            StatusCode::GONE
        );
    }

    #[test]
    fn test_conflict_is_409() {
        assert_eq!(
            status(AppError::conflict("duplicate", json!({}))),
            StatusCode::CONFLICT
        );
    }

    #[test]
    fn test_unauthorized_is_401() {
        assert_eq!(
            status(AppError::unauthorized("token invalid", json!({}))),
            StatusCode::UNAUTHORIZED
        );
    }

    #[test]
    fn test_internal_is_500() {
        assert_eq!(
            status(AppError::internal("oops", json!({}))),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    // ── Unauthorized includes WWW-Authenticate header ─────────────────────────

    #[test]
    fn test_unauthorized_has_www_authenticate_header() {
        let response = AppError::unauthorized("bad token", json!({})).into_response();
        let www_auth = response.headers().get(axum::http::header::WWW_AUTHENTICATE);
        assert!(
            www_auth.is_some(),
            "WWW-Authenticate header must be present"
        );
        assert_eq!(www_auth.unwrap(), "Bearer");
    }

    #[test]
    fn test_other_errors_have_no_www_authenticate_header() {
        for err in [
            AppError::bad_request("x", json!({})),
            AppError::not_found("x", json!({})),
            AppError::gone("x", json!({})),
            AppError::conflict("x", json!({})),
            AppError::internal("x", json!({})),
        ] {
            let response = err.into_response();
            assert!(
                response
                    .headers()
                    .get(axum::http::header::WWW_AUTHENTICATE)
                    .is_none(),
                "WWW-Authenticate must not appear for non-Unauthorized errors"
            );
        }
    }

    // ── to_error_info codes ───────────────────────────────────────────────────

    #[test]
    fn test_to_error_info_codes() {
        assert_eq!(
            AppError::bad_request("x", json!({})).to_error_info().code,
            "validation_error"
        );
        assert_eq!(
            AppError::not_found("x", json!({})).to_error_info().code,
            "not_found"
        );
        assert_eq!(AppError::gone("x", json!({})).to_error_info().code, "gone");
        assert_eq!(
            AppError::conflict("x", json!({})).to_error_info().code,
            "conflict"
        );
        assert_eq!(
            AppError::unauthorized("x", json!({})).to_error_info().code,
            "unauthorized"
        );
        assert_eq!(
            AppError::internal("x", json!({})).to_error_info().code,
            "internal_error"
        );
    }

    // ── Display ───────────────────────────────────────────────────────────────

    #[test]
    fn test_display_includes_message() {
        assert!(
            AppError::bad_request("bad input", json!({}))
                .to_string()
                .contains("bad input")
        );
        assert!(
            AppError::not_found("missing", json!({}))
                .to_string()
                .contains("missing")
        );
        assert!(
            AppError::gone("deleted", json!({}))
                .to_string()
                .contains("deleted")
        );
        assert!(
            AppError::conflict("dup", json!({}))
                .to_string()
                .contains("dup")
        );
        assert!(
            AppError::unauthorized("denied", json!({}))
                .to_string()
                .contains("denied")
        );
        assert!(
            AppError::internal("crash", json!({}))
                .to_string()
                .contains("crash")
        );
    }
}
