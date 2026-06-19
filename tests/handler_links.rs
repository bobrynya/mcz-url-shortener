mod common;

use axum::{
    Router,
    routing::{delete, patch},
};
use axum_test::TestServer;
use serde_json::json;
use sqlx::PgPool;
use url_shortener::api::handlers::{delete_link_handler, update_link_handler};

/// Build a test server with update and delete link routes.
///
/// Both handlers call `extract_domain_from_headers`, which reads the `Host`
/// header.  In every test we set `Host: s.example.com` — the default domain
/// seeded by migrations.
fn make_server(pool: PgPool) -> TestServer {
    let state = common::create_test_state(pool);
    let app = Router::new()
        .route("/api/links/{code}", patch(update_link_handler))
        .route("/api/links/{code}", delete(delete_link_handler))
        .with_state(state);
    TestServer::new(app).unwrap()
}

// ─── DELETE ──────────────────────────────────────────────────────────────────

#[sqlx::test]
async fn test_delete_link_success(pool: PgPool) {
    let domain_id = common::get_default_domain(&pool).await;
    common::create_test_link(&pool, "del001", "https://example.com", domain_id).await;

    let server = make_server(pool);
    let response = server
        .delete("/api/links/del001")
        .add_header("Host", "s.example.com")
        .await;

    response.assert_status(axum::http::StatusCode::NO_CONTENT);
}

#[sqlx::test]
async fn test_delete_link_not_found(pool: PgPool) {
    let server = make_server(pool);
    let response = server
        .delete("/api/links/nonexistent")
        .add_header("Host", "s.example.com")
        .await;

    response.assert_status_not_found();
}

#[sqlx::test]
async fn test_delete_link_already_deleted(pool: PgPool) {
    let domain_id = common::get_default_domain(&pool).await;
    common::create_test_link(&pool, "del002", "https://example.com", domain_id).await;

    let server = make_server(pool);

    // First delete succeeds.
    server
        .delete("/api/links/del002")
        .add_header("Host", "s.example.com")
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);

    // Second delete returns 404 — already deleted.
    server
        .delete("/api/links/del002")
        .add_header("Host", "s.example.com")
        .await
        .assert_status_not_found();
}

#[sqlx::test]
async fn test_delete_link_missing_host_header(pool: PgPool) {
    let domain_id = common::get_default_domain(&pool).await;
    common::create_test_link(&pool, "del003", "https://example.com", domain_id).await;

    let server = make_server(pool);
    // No Host header — expect 400 Bad Request.
    let response = server.delete("/api/links/del003").await;

    response.assert_status_bad_request();
}

// ─── PATCH (update) ───────────────────────────────────────────────────────────

#[sqlx::test]
async fn test_update_link_url(pool: PgPool) {
    let domain_id = common::get_default_domain(&pool).await;
    common::create_test_link(&pool, "upd001", "https://old.com", domain_id).await;

    let server = make_server(pool);
    let response = server
        .patch("/api/links/upd001")
        .add_header("Host", "s.example.com")
        .json(&json!({ "url": "https://new.com" }))
        .await;

    response.assert_status_ok();

    let body = response.json::<serde_json::Value>();
    assert_eq!(body["long_url"], "https://new.com");
    assert_eq!(body["code"], "upd001");
}

#[sqlx::test]
async fn test_update_link_permanent_flag(pool: PgPool) {
    let domain_id = common::get_default_domain(&pool).await;
    common::create_test_link(&pool, "upd002", "https://example.com", domain_id).await;

    let server = make_server(pool);
    let response = server
        .patch("/api/links/upd002")
        .add_header("Host", "s.example.com")
        .json(&json!({ "permanent": true }))
        .await;

    response.assert_status_ok();

    let body = response.json::<serde_json::Value>();
    assert_eq!(body["permanent"], true);
}

#[sqlx::test]
async fn test_update_link_expires_at(pool: PgPool) {
    let domain_id = common::get_default_domain(&pool).await;
    common::create_test_link(&pool, "upd003", "https://example.com", domain_id).await;

    let server = make_server(pool);
    let response = server
        .patch("/api/links/upd003")
        .add_header("Host", "s.example.com")
        .json(&json!({ "expires_at": "2099-12-31T23:59:59Z" }))
        .await;

    response.assert_status_ok();

    let body = response.json::<serde_json::Value>();
    assert!(body["expires_at"].is_string());
    assert!(body["expires_at"].as_str().unwrap().starts_with("2099"));
}

#[sqlx::test]
async fn test_update_link_clear_expires_at(pool: PgPool) {
    let domain_id = common::get_default_domain(&pool).await;
    common::create_test_link(&pool, "upd004", "https://example.com", domain_id).await;

    let server = make_server(pool);

    // Set an expiry first.
    server
        .patch("/api/links/upd004")
        .add_header("Host", "s.example.com")
        .json(&json!({ "expires_at": "2099-12-31T23:59:59Z" }))
        .await
        .assert_status_ok();

    // Clear it with null.
    let response = server
        .patch("/api/links/upd004")
        .add_header("Host", "s.example.com")
        .json(&json!({ "expires_at": null }))
        .await;

    response.assert_status_ok();

    let body = response.json::<serde_json::Value>();
    assert!(body["expires_at"].is_null());
}

#[sqlx::test]
async fn test_update_link_restore(pool: PgPool) {
    let domain_id = common::get_default_domain(&pool).await;
    common::create_test_link(&pool, "upd005", "https://example.com", domain_id).await;

    let server = make_server(pool);

    // Delete the link first.
    server
        .delete("/api/links/upd005")
        .add_header("Host", "s.example.com")
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);

    // Restore it via PATCH.
    let response = server
        .patch("/api/links/upd005")
        .add_header("Host", "s.example.com")
        .json(&json!({ "restore": true }))
        .await;

    response.assert_status_ok();

    let body = response.json::<serde_json::Value>();
    assert!(body["deleted_at"].is_null());
}

#[sqlx::test]
async fn test_update_link_not_found(pool: PgPool) {
    let server = make_server(pool);
    let response = server
        .patch("/api/links/ghost")
        .add_header("Host", "s.example.com")
        .json(&json!({ "url": "https://new.com" }))
        .await;

    response.assert_status_not_found();
}

#[sqlx::test]
async fn test_update_link_invalid_url(pool: PgPool) {
    let domain_id = common::get_default_domain(&pool).await;
    common::create_test_link(&pool, "upd006", "https://example.com", domain_id).await;

    let server = make_server(pool);
    let response = server
        .patch("/api/links/upd006")
        .add_header("Host", "s.example.com")
        .json(&json!({ "url": "not-a-url" }))
        .await;

    response.assert_status_bad_request();

    let body = response.json::<serde_json::Value>();
    assert_eq!(body["error"]["code"], "validation_error");
}

#[sqlx::test]
async fn test_update_link_response_shape(pool: PgPool) {
    let domain_id = common::get_default_domain(&pool).await;
    common::create_test_link(&pool, "upd007", "https://example.com", domain_id).await;

    let server = make_server(pool);
    let response = server
        .patch("/api/links/upd007")
        .add_header("Host", "s.example.com")
        .json(&json!({ "url": "https://updated.com" }))
        .await;

    response.assert_status_ok();

    let body = response.json::<serde_json::Value>();
    assert!(body.get("code").is_some());
    assert!(body.get("long_url").is_some());
    assert!(body.get("short_url").is_some());
    assert!(body.get("permanent").is_some());
    assert!(body.get("created_at").is_some());
}
