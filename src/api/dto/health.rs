//! DTOs for health check endpoint.

use serde::Serialize;

/// Health check response with component status.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub checks: HealthChecks,
}

/// Health status for each system component.
#[derive(Debug, Serialize)]
pub struct HealthChecks {
    pub database: CheckStatus,
    pub cache: CheckStatus,
    pub kafka: CheckStatus,
    pub clickhouse: CheckStatus,
}

/// Individual component health status.
#[derive(Debug, Serialize)]
pub struct CheckStatus {
    pub status: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}
