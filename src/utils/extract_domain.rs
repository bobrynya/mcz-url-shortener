use crate::AppError;
use axum::http::{HeaderMap, header};

pub fn extract_domain_from_headers(headers: &HeaderMap) -> Result<String, AppError> {
    let host = headers
        .get(header::HOST)
        .ok_or_else(|| AppError::bad_request("Missing Host header", serde_json::json!({})))?
        .to_str()
        .map_err(|_| AppError::bad_request("Invalid Host header", serde_json::json!({})))?;

    // Убираем порт, если есть (example.com:3000 -> example.com)
    let domain = host.split(':').next().unwrap_or(host);

    Ok(domain.to_string())
}
