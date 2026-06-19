mod common;

use axum::{Router, routing::get};
use axum_test::TestServer;
use sqlx::PgPool;
use url_shortener::api::handlers::health_handler;

#[sqlx::test]
async fn test_health_endpoint_degraded_when_non_critical_down(pool: PgPool) {
    // Postgres is up, but Kafka/ClickHouse/cache are unconfigured in tests.
    // Only the database is critical, so the endpoint stays 200 and reports
    // `degraded`.
    let state = common::create_test_state(pool);
    let app = Router::new()
        .route("/api/health", get(health_handler))
        .with_state(state);

    let server = TestServer::new(app).unwrap();

    let response = server.get("/api/health").await;

    // Non-critical deps down → still 200.
    response.assert_status_ok();

    let json = response.json::<serde_json::Value>();
    assert_eq!(json["status"], "degraded");
    // Database is the only critical check and is up.
    assert_eq!(json["checks"]["database"]["status"], "ok");
    // Kafka & ClickHouse are unconfigured here → reported as errors.
    assert_eq!(json["checks"]["kafka"]["status"], "error");
    assert_eq!(json["checks"]["clickhouse"]["status"], "error");
}

#[sqlx::test]
async fn test_health_endpoint_structure(pool: PgPool) {
    let state = common::create_test_state(pool);
    let app = Router::new()
        .route("/api/health", get(health_handler))
        .with_state(state);

    let server = TestServer::new(app).unwrap();

    let response = server.get("/api/health").await;

    let json = response.json::<serde_json::Value>();

    assert!(json.get("status").is_some());
    assert!(json.get("version").is_some());
    assert!(json.get("checks").is_some());
    assert!(json["checks"].get("database").is_some());
    assert!(json["checks"].get("cache").is_some());
    // `click_queue` is gone; clicks now flow through Kafka + ClickHouse.
    assert!(json["checks"].get("kafka").is_some());
    assert!(json["checks"].get("clickhouse").is_some());
    assert!(json["checks"].get("click_queue").is_none());
}

// NOTE: Asserting a 503 requires the database (the only critical dependency) to
// be DOWN. The test harness builds `AppState` against a live `sqlx::test` pool,
// so there is no in-process way to simulate a DB failure here. The 503-on-DB-down
// path is exercised manually / in higher-level smoke tests.
