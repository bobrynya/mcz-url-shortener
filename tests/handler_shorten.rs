mod common;

use axum::{Router, routing::post};
use axum_test::TestServer;
use serde_json::json;
use sqlx::PgPool;
use url_shortener::api::handlers::shorten_handler;

#[sqlx::test]
async fn test_shorten_single_url_success(pool: PgPool) {
    let (state, _rx) = common::create_test_state(pool);
    let app = Router::new()
        .route("/api/shorten", post(shorten_handler))
        .with_state(state);

    let server = TestServer::new(app).unwrap();

    let response = server
        .post("/api/shorten")
        .json(&json!({
            "urls": [
                {
                    "url": "https://example.com"
                }
            ]
        }))
        .await;

    response.assert_status_ok();

    let json = response.json::<serde_json::Value>();
    assert_eq!(json["summary"]["total"], 1);
    assert_eq!(json["summary"]["successful"], 1);
    assert_eq!(json["summary"]["failed"], 0);

    let items = json["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert!(items[0]["code"].is_string());
    assert!(items[0]["short_url"].is_string());
    assert_eq!(items[0]["long_url"], "https://example.com");
}

#[sqlx::test]
async fn test_shorten_with_custom_code(pool: PgPool) {
    let (state, _rx) = common::create_test_state(pool);
    let app = Router::new()
        .route("/api/shorten", post(shorten_handler))
        .with_state(state);

    let server = TestServer::new(app).unwrap();

    let response = server
        .post("/api/shorten")
        .json(&json!({
            "urls": [
                {
                    "url": "https://example.com",
                    "custom_code": "mycode123"
                }
            ]
        }))
        .await;

    response.assert_status_ok();

    let json = response.json::<serde_json::Value>();
    assert_eq!(json["summary"]["successful"], 1);

    let items = json["items"].as_array().unwrap();
    assert_eq!(items[0]["code"], "mycode123");
}

#[sqlx::test]
async fn test_shorten_multiple_urls(pool: PgPool) {
    let (state, _rx) = common::create_test_state(pool);
    let app = Router::new()
        .route("/api/shorten", post(shorten_handler))
        .with_state(state);

    let server = TestServer::new(app).unwrap();

    let response = server
        .post("/api/shorten")
        .json(&json!({
            "urls": [
                { "url": "https://example.com/1" },
                { "url": "https://example.com/2" },
                { "url": "https://example.com/3" }
            ]
        }))
        .await;

    response.assert_status_ok();

    let json = response.json::<serde_json::Value>();
    assert_eq!(json["summary"]["total"], 3);
    assert_eq!(json["summary"]["successful"], 3);
    assert_eq!(json["summary"]["failed"], 0);

    let items = json["items"].as_array().unwrap();
    assert_eq!(items.len(), 3);
}

#[sqlx::test]
async fn test_shorten_deduplication(pool: PgPool) {
    let (state, _rx) = common::create_test_state(pool);
    let app = Router::new()
        .route("/api/shorten", post(shorten_handler))
        .with_state(state);

    let server = TestServer::new(app).unwrap();
    let response1 = server
        .post("/api/shorten")
        .json(&json!({
            "urls": [{ "url": "https://dedup.com" }]
        }))
        .await;

    let json1 = response1.json::<serde_json::Value>();
    let code1 = json1["items"][0]["code"].as_str().unwrap();

    let response2 = server
        .post("/api/shorten")
        .json(&json!({
            "urls": [{ "url": "https://dedup.com" }]
        }))
        .await;

    let json2 = response2.json::<serde_json::Value>();
    let code2 = json2["items"][0]["code"].as_str().unwrap();
    assert_eq!(code1, code2);
}

#[sqlx::test]
async fn test_shorten_invalid_url(pool: PgPool) {
    let (state, _rx) = common::create_test_state(pool);
    let app = Router::new()
        .route("/api/shorten", post(shorten_handler))
        .with_state(state);

    let server = TestServer::new(app).unwrap();
    let response = server
        .post("/api/shorten")
        .json(&json!({
            "urls": [{ "url": "not-a-valid-url" }]
        }))
        .await;
    response.assert_status_bad_request();

    let json = response.json::<serde_json::Value>();
    assert_eq!(json["error"]["code"], "validation_error");
}

#[sqlx::test]
async fn test_shorten_rejects_private_url(pool: PgPool) {
    // The shared test state mirrors production (block_private_urls = true). A
    // localhost URL is a syntactically valid URL (passes DTO validation) but is
    // rejected by the service, surfacing as a per-item error in the batch.
    let (state, _rx) = common::create_test_state(pool);
    let app = Router::new()
        .route("/api/shorten", post(shorten_handler))
        .with_state(state);

    let server = TestServer::new(app).unwrap();
    let response = server
        .post("/api/shorten")
        .json(&json!({
            "urls": [{ "url": "http://localhost:8080/admin" }]
        }))
        .await;

    response.assert_status_ok();

    let json = response.json::<serde_json::Value>();
    assert_eq!(json["summary"]["successful"], 0);
    assert_eq!(json["summary"]["failed"], 1);

    let items = json["items"].as_array().unwrap();
    assert_eq!(items[0]["error"]["code"], "validation_error");
}

#[sqlx::test]
async fn test_shorten_custom_code_conflict(pool: PgPool) {
    let (state, _rx) = common::create_test_state(pool);
    let app = Router::new()
        .route("/api/shorten", post(shorten_handler))
        .with_state(state);

    let server = TestServer::new(app).unwrap();
    server
        .post("/api/shorten")
        .json(&json!({
            "urls": [
                {
                    "url": "https://first.com",
                    "custom_code": "taken123"
                }
            ]
        }))
        .await
        .assert_status_ok();

    let response = server
        .post("/api/shorten")
        .json(&json!({
            "urls": [
                {
                    "url": "https://second.com",
                    "custom_code": "taken123"
                }
            ]
        }))
        .await;

    response.assert_status_ok();

    let json = response.json::<serde_json::Value>();
    assert_eq!(json["summary"]["failed"], 1);

    let items = json["items"].as_array().unwrap();
    assert!(items[0].get("error").is_some());
}

#[sqlx::test]
async fn test_shorten_mixed_success_and_failure(pool: PgPool) {
    let (state, _rx) = common::create_test_state(pool);
    let app = Router::new()
        .route("/api/shorten", post(shorten_handler))
        .with_state(state);

    let server = TestServer::new(app).unwrap();
    let response = server
        .post("/api/shorten")
        .json(&json!({
            "urls": [
                { "url": "https://valid.com" },
                { "url": "invalid-url" },
                { "url": "https://another-valid.com" }
            ]
        }))
        .await;
    response.assert_status_bad_request();

    let json = response.json::<serde_json::Value>();
    assert_eq!(json["error"]["code"], "validation_error");
}

#[sqlx::test]
async fn test_shorten_url_normalization(pool: PgPool) {
    let (state, _rx) = common::create_test_state(pool);
    let app = Router::new()
        .route("/api/shorten", post(shorten_handler))
        .with_state(state);

    let server = TestServer::new(app).unwrap();
    let response = server
        .post("/api/shorten")
        .json(&json!({
            "urls": [
                { "url": "https://EXAMPLE.COM:443/path" }
            ]
        }))
        .await;

    response.assert_status_ok();

    let json = response.json::<serde_json::Value>();
    assert_eq!(json["summary"]["successful"], 1);

    let items = json["items"].as_array().unwrap();
    let short_url = items[0]["short_url"].as_str().unwrap();
    assert!(short_url.contains("s.example.com"));
}

#[sqlx::test]
async fn test_shorten_partial_failure_reserved_custom_code(pool: PgPool) {
    let (state, _rx) = common::create_test_state(pool);
    let app = Router::new()
        .route("/api/shorten", post(shorten_handler))
        .with_state(state);

    let server = TestServer::new(app).unwrap();
    let response = server
        .post("/api/shorten")
        .json(&json!({
            "urls": [
                { "url": "https://valid.com" },
                {
                    "url": "https://another.com",
                    "custom_code": "admin"
                }
            ]
        }))
        .await;

    response.assert_status_ok();

    let json = response.json::<serde_json::Value>();
    assert_eq!(json["summary"]["total"], 2);
    assert_eq!(json["summary"]["successful"], 1);
    assert_eq!(json["summary"]["failed"], 1);

    let items = json["items"].as_array().unwrap();
    assert!(items[0].get("code").is_some());
    assert!(items[1].get("error").is_some());
    assert_eq!(items[1]["error"]["code"], "validation_error");
}
