mod common;

use axum::{
    Router,
    routing::{delete, get, patch, post},
};
use axum_test::TestServer;
use serde_json::json;
use sqlx::PgPool;
use url_shortener::api::handlers::{
    create_domain_handler, delete_domain_handler, domain_list_handler, update_domain_handler,
};

fn make_server(pool: PgPool) -> TestServer {
    let state = common::create_test_state(pool);
    let app = Router::new()
        .route("/api/domains", get(domain_list_handler))
        .route("/api/domains", post(create_domain_handler))
        .route("/api/domains/{id}", patch(update_domain_handler))
        .route("/api/domains/{id}", delete(delete_domain_handler))
        .with_state(state);
    TestServer::new(app).unwrap()
}

// ─── LIST ────────────────────────────────────────────────────────────────────

#[sqlx::test]
async fn test_domains_list_success(pool: PgPool) {
    let server = make_server(pool.clone());

    common::create_test_domain(&pool, "test1.com").await;
    common::create_test_domain(&pool, "test2.com").await;

    let response = server.get("/api/domains").await;

    response.assert_status_ok();

    let json = response.json::<serde_json::Value>();
    let items = json["items"].as_array().unwrap();

    assert!(items.len() >= 3);

    assert!(items[0].get("domain").is_some());
    assert!(items[0].get("is_default").is_some());
    assert!(items[0].get("is_active").is_some());
}

#[sqlx::test]
async fn test_domains_list_has_default(pool: PgPool) {
    let server = make_server(pool);

    let response = server.get("/api/domains").await;

    response.assert_status_ok();

    let json = response.json::<serde_json::Value>();
    let items = json["items"].as_array().unwrap();

    let has_default = items.iter().any(|item| item["is_default"] == true);
    assert!(has_default);
}

#[sqlx::test]
async fn test_domains_list_structure(pool: PgPool) {
    let server = make_server(pool);

    let response = server.get("/api/domains").await;

    response.assert_status_ok();

    let json = response.json::<serde_json::Value>();

    assert!(json.get("items").is_some());
    assert!(json["items"].is_array());

    let items = json["items"].as_array().unwrap();
    if !items.is_empty() {
        let first = &items[0];
        assert!(first.get("domain").is_some());
        assert!(first.get("is_default").is_some());
        assert!(first.get("is_active").is_some());
        assert!(first.get("created_at").is_some());
        assert!(first.get("updated_at").is_some());
    }
}

// ─── CREATE ───────────────────────────────────────────────────────────────────

#[sqlx::test]
async fn test_create_domain_success(pool: PgPool) {
    let server = make_server(pool);

    let response = server
        .post("/api/domains")
        .json(&json!({ "domain": "newdomain.com" }))
        .await;

    response.assert_status(axum::http::StatusCode::CREATED);

    let body = response.json::<serde_json::Value>();
    assert_eq!(body["domain"], "newdomain.com");
    assert_eq!(body["is_default"], false);
    assert_eq!(body["is_active"], true);
    assert!(body.get("id").is_some());
}

#[sqlx::test]
async fn test_create_domain_with_description(pool: PgPool) {
    let server = make_server(pool);

    let response = server
        .post("/api/domains")
        .json(&json!({
            "domain": "described.com",
            "description": "My custom domain"
        }))
        .await;

    response.assert_status(axum::http::StatusCode::CREATED);

    let body = response.json::<serde_json::Value>();
    assert_eq!(body["description"], "My custom domain");
}

/// Creating a domain with `is_default: true` when a default already exists
/// conflicts with the unique constraint on the default flag and returns 409.
/// To change the default domain, use `PATCH /api/domains/{id}` with
/// `{ "is_default": true }` on an existing domain instead.
#[sqlx::test]
async fn test_create_domain_with_is_default_conflicts(pool: PgPool) {
    let server = make_server(pool);

    let response = server
        .post("/api/domains")
        .json(&json!({
            "domain": "newdefault.com",
            "is_default": true
        }))
        .await;

    response.assert_status(axum::http::StatusCode::CONFLICT);
}

#[sqlx::test]
async fn test_create_domain_duplicate(pool: PgPool) {
    let server = make_server(pool);

    server
        .post("/api/domains")
        .json(&json!({ "domain": "dup.com" }))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    // Same domain a second time — expect 409 Conflict.
    let response = server
        .post("/api/domains")
        .json(&json!({ "domain": "dup.com" }))
        .await;

    response.assert_status(axum::http::StatusCode::CONFLICT);
}

// ─── UPDATE ───────────────────────────────────────────────────────────────────

#[sqlx::test]
async fn test_update_domain_description(pool: PgPool) {
    let id = common::create_test_domain(&pool, "edit-me.com").await;
    let server = make_server(pool);

    let response = server
        .patch(&format!("/api/domains/{id}"))
        .json(&json!({ "description": "Updated desc" }))
        .await;

    response.assert_status_ok();

    let body = response.json::<serde_json::Value>();
    assert_eq!(body["description"], "Updated desc");
    assert_eq!(body["domain"], "edit-me.com");
}

#[sqlx::test]
async fn test_update_domain_deactivate(pool: PgPool) {
    let id = common::create_test_domain(&pool, "deactivate-me.com").await;
    let server = make_server(pool);

    let response = server
        .patch(&format!("/api/domains/{id}"))
        .json(&json!({ "is_active": false }))
        .await;

    response.assert_status_ok();

    let body = response.json::<serde_json::Value>();
    assert_eq!(body["is_active"], false);
}

#[sqlx::test]
async fn test_update_domain_set_default(pool: PgPool) {
    let id = common::create_test_domain(&pool, "become-default.com").await;
    let server = make_server(pool);

    let response = server
        .patch(&format!("/api/domains/{id}"))
        .json(&json!({ "is_default": true }))
        .await;

    response.assert_status_ok();

    let body = response.json::<serde_json::Value>();
    assert_eq!(body["is_default"], true);
}

#[sqlx::test]
async fn test_update_domain_not_found(pool: PgPool) {
    let server = make_server(pool);

    let response = server
        .patch("/api/domains/999999")
        .json(&json!({ "description": "ghost" }))
        .await;

    response.assert_status_not_found();
}

// ─── DELETE ───────────────────────────────────────────────────────────────────

#[sqlx::test]
async fn test_delete_domain_success(pool: PgPool) {
    let id = common::create_test_domain(&pool, "bye.com").await;
    let server = make_server(pool);

    let response = server.delete(&format!("/api/domains/{id}")).await;

    response.assert_status(axum::http::StatusCode::NO_CONTENT);
}

#[sqlx::test]
async fn test_delete_domain_default_rejected(pool: PgPool) {
    let default_id = common::get_default_domain(&pool).await;
    let server = make_server(pool);

    // Deleting the default domain must be rejected.
    let response = server.delete(&format!("/api/domains/{default_id}")).await;

    // Expect 4xx — the service returns an error for default domain deletion.
    assert!(
        response.status_code().is_client_error(),
        "expected client error, got {}",
        response.status_code()
    );
}

#[sqlx::test]
async fn test_delete_domain_not_found(pool: PgPool) {
    let server = make_server(pool);

    let response = server.delete("/api/domains/999999").await;

    response.assert_status_not_found();
}
