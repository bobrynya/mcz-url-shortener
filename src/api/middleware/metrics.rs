//! HTTP RED metrics middleware: request count + latency, low-cardinality labels.

use std::time::Instant;

use axum::extract::{MatchedPath, Request};
use axum::middleware::Next;
use axum::response::Response;

/// Returns the low-cardinality route-template label for a request.
///
/// Uses the matched route pattern (e.g. `/{code}`, `/api/links/{code}`), never
/// the raw URI — otherwise every short code becomes its own time series. Requests
/// that reached this layer without a matched path are labelled `"<unmatched>"`.
pub fn matched_path_label(matched: Option<&MatchedPath>) -> String {
    matched
        .map(|m| m.as_str().to_owned())
        .unwrap_or_else(|| "<unmatched>".to_owned())
}

/// Records `http_requests_total` and `http_request_duration_seconds` per request.
///
/// `/metrics` is skipped so Prometheus scrapes don't inflate the HTTP metrics.
pub async fn track_metrics(req: Request, next: Next) -> Response {
    let path = matched_path_label(req.extensions().get::<MatchedPath>());

    if path == "/metrics" {
        return next.run(req).await;
    }

    let method = req.method().as_str().to_owned();
    let start = Instant::now();
    let response = next.run(req).await;
    let elapsed = start.elapsed().as_secs_f64();
    let status = response.status().as_u16().to_string();

    metrics::counter!(
        "http_requests_total",
        "method" => method.clone(),
        "path" => path.clone(),
        "status" => status.clone(),
    )
    .increment(1);
    metrics::histogram!(
        "http_request_duration_seconds",
        "method" => method,
        "path" => path,
        "status" => status,
    )
    .record(elapsed);

    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_falls_back_when_no_matched_path() {
        // `MatchedPath` has no public constructor; the populated case is covered
        // by the integration test asserting `path="/{code}"`. Here we pin the
        // fallback used for unmatched requests.
        assert_eq!(matched_path_label(None), "<unmatched>");
    }
}
