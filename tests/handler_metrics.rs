mod common;

use axum::Router;
use axum::routing::get;
use axum_test::TestServer;
use sqlx::PgPool;
use url_shortener::observability::metrics::metrics_handler;

#[sqlx::test]
async fn metrics_endpoint_returns_prometheus_exposition(pool: PgPool) {
    let state = common::create_test_state(pool);
    let app = Router::new()
        .route("/metrics", get(metrics_handler))
        .with_state(state);
    let server = TestServer::new(app).unwrap();

    let response = server.get("/metrics").await;

    response.assert_status_ok();
    let content_type = response.header("content-type");
    let content_type = content_type.to_str().unwrap();
    assert!(
        content_type.starts_with("text/plain"),
        "unexpected content-type: {content_type}"
    );
}

#[sqlx::test]
async fn http_metrics_use_route_template_and_exclude_self(pool: PgPool) {
    use url_shortener::api::middleware::metrics::track_metrics;

    let state = common::create_test_state(pool);
    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/{code}", get(|| async { "ok" }))
        .route("/metrics", get(metrics_handler))
        .route_layer(axum::middleware::from_fn(track_metrics))
        .with_state(state);
    let server = TestServer::new(app).unwrap();

    server.get("/health").await;
    server.get("/abc123XYZ").await;
    let body = server.get("/metrics").await.text();

    assert!(
        body.contains("http_requests_total"),
        "missing http_requests_total in:\n{body}"
    );
    // Route template label, not the raw code:
    assert!(
        body.contains(r#"path="/{code}""#),
        "missing templated path label in:\n{body}"
    );
    assert!(
        !body.contains("abc123XYZ"),
        "raw short code leaked into a label in:\n{body}"
    );
    // /metrics must not instrument itself:
    assert!(
        !body.contains(r#"path="/metrics""#),
        "/metrics was self-instrumented in:\n{body}"
    );
}

#[sqlx::test]
async fn links_created_total_increments_on_create(pool: PgPool) {
    use url_shortener::observability::metrics::install_prometheus_recorder;

    let handle = install_prometheus_recorder();
    let state = common::create_test_state(pool);

    state
        .link_service
        .create_short_link("https://example.com/page".to_owned(), None, None, false)
        .await
        .unwrap();

    let body = handle.render();
    assert!(
        body.contains("links_created_total"),
        "missing links_created_total in:\n{body}"
    );
}

#[test]
fn grafana_dashboard_json_is_valid() {
    let raw = std::fs::read_to_string("dashboards/url-shortener.json")
        .expect("dashboards/url-shortener.json must exist");
    let value: serde_json::Value =
        serde_json::from_str(&raw).expect("dashboard must be valid JSON");
    assert!(value.get("panels").and_then(|p| p.as_array()).is_some());
    assert!(
        raw.contains("${DS_PROMETHEUS}"),
        "dashboard must use the DS_PROMETHEUS datasource variable"
    );
}
