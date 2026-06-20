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
/// Both handlers resolve domain via `domain_id`: PATCH reads it from the JSON
/// body; DELETE reads it from the `?domain_id=` query parameter. When omitted,
/// the default domain is used. No `Host` header is consulted.
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
        .delete(&format!("/api/links/del001?domain_id={domain_id}"))
        .await;

    response.assert_status(axum::http::StatusCode::NO_CONTENT);
}

#[sqlx::test]
async fn test_delete_link_not_found(pool: PgPool) {
    let domain_id = common::get_default_domain(&pool).await;

    let server = make_server(pool);
    let response = server
        .delete(&format!("/api/links/nonexistent?domain_id={domain_id}"))
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
        .delete(&format!("/api/links/del002?domain_id={domain_id}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);

    // Second delete returns 404 — already deleted.
    server
        .delete(&format!("/api/links/del002?domain_id={domain_id}"))
        .await
        .assert_status_not_found();
}

#[sqlx::test]
async fn test_delete_link_unknown_domain_id(pool: PgPool) {
    let server = make_server(pool);
    let response = server.delete("/api/links/whatever?domain_id=999999").await;
    response.assert_status_not_found();
}

#[sqlx::test]
async fn test_delete_link_defaults_to_default_domain(pool: PgPool) {
    let domain_id = common::get_default_domain(&pool).await;
    common::create_test_link(&pool, "del010", "https://example.com", domain_id).await;

    let server = make_server(pool);
    // No domain_id → default domain.
    server
        .delete("/api/links/del010")
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);
}

// ─── PATCH (update) ───────────────────────────────────────────────────────────

#[sqlx::test]
async fn test_update_link_url(pool: PgPool) {
    let domain_id = common::get_default_domain(&pool).await;
    common::create_test_link(&pool, "upd001", "https://old.com", domain_id).await;

    let server = make_server(pool);
    let response = server
        .patch("/api/links/upd001")
        .json(&json!({ "domain_id": domain_id, "url": "https://new.com" }))
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
        .json(&json!({ "domain_id": domain_id, "permanent": true }))
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
        .json(&json!({ "domain_id": domain_id, "expires_at": "2099-12-31T23:59:59Z" }))
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
        .json(&json!({ "domain_id": domain_id, "expires_at": "2099-12-31T23:59:59Z" }))
        .await
        .assert_status_ok();

    // Clear it with null.
    let response = server
        .patch("/api/links/upd004")
        .json(&json!({ "domain_id": domain_id, "expires_at": null }))
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
        .delete(&format!("/api/links/upd005?domain_id={domain_id}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);

    // Restore it via PATCH.
    let response = server
        .patch("/api/links/upd005")
        .json(&json!({ "domain_id": domain_id, "restore": true }))
        .await;

    response.assert_status_ok();

    let body = response.json::<serde_json::Value>();
    assert!(body["deleted_at"].is_null());
}

#[sqlx::test]
async fn test_update_link_not_found(pool: PgPool) {
    let domain_id = common::get_default_domain(&pool).await;

    let server = make_server(pool);
    let response = server
        .patch("/api/links/ghost")
        .json(&json!({ "domain_id": domain_id, "url": "https://new.com" }))
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
        .json(&json!({ "domain_id": domain_id, "url": "not-a-url" }))
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
        .json(&json!({ "domain_id": domain_id, "url": "https://updated.com" }))
        .await;

    response.assert_status_ok();

    let body = response.json::<serde_json::Value>();
    assert!(body.get("code").is_some());
    assert!(body.get("long_url").is_some());
    assert!(body.get("short_url").is_some());
    assert!(body.get("permanent").is_some());
    assert!(body.get("created_at").is_some());
}

#[sqlx::test]
async fn test_update_link_unknown_domain_id(pool: PgPool) {
    let server = make_server(pool);
    let response = server
        .patch("/api/links/whatever")
        .json(&json!({ "domain_id": 999999, "url": "https://new.com" }))
        .await;
    response.assert_status_not_found();
}

#[sqlx::test]
async fn test_update_link_defaults_to_default_domain(pool: PgPool) {
    let domain_id = common::get_default_domain(&pool).await;
    common::create_test_link(&pool, "upd010", "https://old.com", domain_id).await;

    let server = make_server(pool);
    // No domain_id in body → default domain.
    let response = server
        .patch("/api/links/upd010")
        .json(&json!({ "url": "https://new.com" }))
        .await;

    response.assert_status_ok();
    let body = response.json::<serde_json::Value>();
    assert_eq!(body["long_url"], "https://new.com");
}
