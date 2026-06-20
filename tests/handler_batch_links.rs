mod common;

use axum::{Router, routing::post};
use axum_test::TestServer;
use serde_json::json;
use sqlx::PgPool;
use url_shortener::api::handlers::{batch_deactivate_handler, batch_restore_handler};

fn make_server(pool: PgPool) -> TestServer {
    let state = common::create_test_state(pool);
    let app = Router::new()
        .route(
            "/api/links/batch-deactivate",
            post(batch_deactivate_handler),
        )
        .route("/api/links/batch-restore", post(batch_restore_handler))
        .with_state(state);
    TestServer::new(app).unwrap()
}

#[sqlx::test]
async fn test_batch_deactivate_partial(pool: PgPool) {
    let domain_id = common::get_default_domain(&pool).await;
    common::create_test_link(&pool, "bx001", "https://a.com", domain_id).await;
    common::create_test_link(&pool, "bx002", "https://b.com", domain_id).await;
    common::create_deleted_link(&pool, "bx003", "https://c.com", domain_id).await;

    let server = make_server(pool);
    let response = server
        .post("/api/links/batch-deactivate")
        .json(&json!({
            "codes": ["bx001", "bx002", "bx003", "bx404"],
            "domain_id": domain_id
        }))
        .await;

    response.assert_status_ok();
    let body = response.json::<serde_json::Value>();
    assert_eq!(body["summary"]["total"], 4);
    assert_eq!(body["summary"]["deactivated"], 2);
    assert_eq!(body["summary"]["not_found"], 2);
    // items preserve input order.
    assert_eq!(body["items"][0]["code"], "bx001");
    assert_eq!(body["items"][0]["status"], "deactivated");
    assert_eq!(body["items"][2]["code"], "bx003");
    assert_eq!(body["items"][2]["status"], "not_found");
}

#[sqlx::test]
async fn test_batch_deactivate_idempotent(pool: PgPool) {
    let domain_id = common::get_default_domain(&pool).await;
    common::create_test_link(&pool, "id001", "https://a.com", domain_id).await;

    let server = make_server(pool);
    let req = json!({ "codes": ["id001"], "domain_id": domain_id });

    server
        .post("/api/links/batch-deactivate")
        .json(&req)
        .await
        .assert_status_ok();

    let second = server.post("/api/links/batch-deactivate").json(&req).await;
    let body = second.json::<serde_json::Value>();
    assert_eq!(body["summary"]["deactivated"], 0);
    assert_eq!(body["summary"]["not_found"], 1);
}

#[sqlx::test]
async fn test_batch_deactivate_defaults_to_default_domain(pool: PgPool) {
    let domain_id = common::get_default_domain(&pool).await;
    common::create_test_link(&pool, "df001", "https://a.com", domain_id).await;

    let server = make_server(pool);
    // No domain_id → default domain.
    let response = server
        .post("/api/links/batch-deactivate")
        .json(&json!({ "codes": ["df001"] }))
        .await;

    response.assert_status_ok();
    let body = response.json::<serde_json::Value>();
    assert_eq!(body["summary"]["deactivated"], 1);
}

#[sqlx::test]
async fn test_batch_restore(pool: PgPool) {
    let domain_id = common::get_default_domain(&pool).await;
    common::create_deleted_link(&pool, "rb001", "https://a.com", domain_id).await;
    common::create_test_link(&pool, "rb002", "https://b.com", domain_id).await; // active → not_found on restore

    let server = make_server(pool);
    let response = server
        .post("/api/links/batch-restore")
        .json(&json!({ "codes": ["rb001", "rb002"], "domain_id": domain_id }))
        .await;

    response.assert_status_ok();
    let body = response.json::<serde_json::Value>();
    assert_eq!(body["summary"]["restored"], 1);
    assert_eq!(body["summary"]["not_found"], 1);
    assert_eq!(body["items"][0]["status"], "restored");
}

#[sqlx::test]
async fn test_batch_deactivate_rejects_empty(pool: PgPool) {
    let server = make_server(pool);
    let response = server
        .post("/api/links/batch-deactivate")
        .json(&json!({ "codes": [] }))
        .await;
    response.assert_status_bad_request();
}

#[sqlx::test]
async fn test_batch_deactivate_rejects_over_limit(pool: PgPool) {
    let server = make_server(pool);
    let codes: Vec<String> = (0..1001).map(|i| format!("c{i}")).collect();
    let response = server
        .post("/api/links/batch-deactivate")
        .json(&json!({ "codes": codes }))
        .await;
    response.assert_status_bad_request();
}

#[sqlx::test]
async fn test_batch_deactivate_unknown_domain_id(pool: PgPool) {
    let server = make_server(pool);
    let response = server
        .post("/api/links/batch-deactivate")
        .json(&json!({ "codes": ["x"], "domain_id": 999999 }))
        .await;
    response.assert_status_not_found();
}

#[sqlx::test]
async fn test_batch_deactivate_dedups_duplicate_codes(pool: PgPool) {
    let domain_id = common::get_default_domain(&pool).await;
    common::create_test_link(&pool, "dup001", "https://a.com", domain_id).await;
    common::create_test_link(&pool, "dup002", "https://b.com", domain_id).await;

    let server = make_server(pool);
    let response = server
        .post("/api/links/batch-deactivate")
        .json(&json!({
            "codes": ["dup001", "dup001", "dup002"],
            "domain_id": domain_id
        }))
        .await;

    response.assert_status_ok();
    let body = response.json::<serde_json::Value>();
    // Duplicate "dup001" collapses to a single item → 2 unique items, both deactivated.
    assert_eq!(body["summary"]["total"], 2);
    assert_eq!(body["summary"]["deactivated"], 2);
    assert_eq!(body["summary"]["not_found"], 0);
    assert_eq!(body["items"].as_array().unwrap().len(), 2);
    assert_eq!(body["items"][0]["code"], "dup001");
    assert_eq!(body["items"][1]["code"], "dup002");
}
