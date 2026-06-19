mod common;

use std::sync::Arc;

use axum::{Router, routing::get};
use axum_test::TestServer;
use chrono::Utc;
use sqlx::PgPool;
use url_shortener::api::handlers::{stats_handler, stats_list_handler};
use url_shortener::domain::entities::Click;
use url_shortener::infrastructure::messaging::NoopClickPublisher;

use common::FakeStatsReader;

/// Builds `n` canned clicks for a link (only the count matters for these tests).
fn canned_clicks(link_id: i64, n: usize) -> Vec<Click> {
    (0..n)
        .map(|i| {
            Click::new(
                i as i64 + 1,
                link_id,
                Utc::now(),
                Some("TestBot/1.0".to_string()),
                None,
                Some(format!("10.0.0.{}", i + 1)),
            )
        })
        .collect()
}

#[sqlx::test]
async fn test_stats_by_code_success(pool: PgPool) {
    let domain_id = common::create_test_domain(&pool, "stats-test.com").await;
    common::create_test_link(&pool, "testcode", "https://example.com", domain_id).await;

    let link_id: i64 = sqlx::query_scalar!("SELECT id FROM links WHERE code = 'testcode'")
        .fetch_one(&pool)
        .await
        .unwrap();

    let reader = FakeStatsReader::new().with_link(link_id, canned_clicks(link_id, 5));
    let state = common::create_test_state_with(
        pool.clone(),
        Arc::new(NoopClickPublisher),
        Arc::new(reader),
    );
    let app = Router::new()
        .route("/api/stats/{code}", get(stats_handler))
        .with_state(state);

    let server = TestServer::new(app).unwrap();

    let response = server.get("/api/stats/testcode").await;

    response.assert_status_ok();

    let json = response.json::<serde_json::Value>();
    assert_eq!(json["code"], "testcode");
    assert_eq!(json["long_url"], "https://example.com");
    assert_eq!(json["total"], 5);
    assert!(json["items"].as_array().unwrap().len() <= 5);
}

#[sqlx::test]
async fn test_stats_by_code_not_found(pool: PgPool) {
    let state = common::create_test_state(pool);
    let app = Router::new()
        .route("/api/stats/{code}", get(stats_handler))
        .with_state(state);

    let server = TestServer::new(app).unwrap();
    let response = server.get("/api/stats/notfound").await;

    response.assert_status_not_found();

    let json = response.json::<serde_json::Value>();
    assert_eq!(json["error"]["code"], "not_found");
}

#[sqlx::test]
async fn test_stats_with_pagination(pool: PgPool) {
    let domain_id = common::create_test_domain(&pool, "paginate-test.com").await;
    common::create_test_link(&pool, "paginate", "https://example.com", domain_id).await;

    let link_id: i64 = sqlx::query_scalar!("SELECT id FROM links WHERE code = 'paginate'")
        .fetch_one(&pool)
        .await
        .unwrap();

    let reader = FakeStatsReader::new().with_link(link_id, canned_clicks(link_id, 15));
    let state = common::create_test_state_with(
        pool.clone(),
        Arc::new(NoopClickPublisher),
        Arc::new(reader),
    );
    let app = Router::new()
        .route("/api/stats/{code}", get(stats_handler))
        .with_state(state);

    let server = TestServer::new(app).unwrap();

    let response = server
        .get("/api/stats/paginate")
        .add_query_param("page_size", "10")
        .await;

    response.assert_status_ok();

    let json = response.json::<serde_json::Value>();
    assert_eq!(json["total"], 15);
    assert_eq!(json["pagination"]["page_size"], 10);
    // FakeStatsReader does not paginate its canned list; the handler reports the
    // ClickHouse-provided total (15) and whatever items the reader returns.
    assert_eq!(json["items"].as_array().unwrap().len(), 15);
}

#[sqlx::test]
async fn test_stats_list_all(pool: PgPool) {
    let domain_id = common::create_test_domain(&pool, "list-test.com").await;

    for i in 1..=3 {
        common::create_test_link(
            &pool,
            &format!("link{}", i),
            &format!("https://example.com/{}", i),
            domain_id,
        )
        .await;
    }

    // No clicks needed; UnavailableStatsReader would 503, so use an empty fake.
    let state = common::create_test_state_with(
        pool.clone(),
        Arc::new(NoopClickPublisher),
        Arc::new(FakeStatsReader::new()),
    );
    let app = Router::new()
        .route("/api/stats", get(stats_list_handler))
        .with_state(state);

    let server = TestServer::new(app).unwrap();

    let response = server.get("/api/stats").await;

    response.assert_status_ok();

    let json = response.json::<serde_json::Value>();
    assert!(json["items"].as_array().unwrap().len() >= 3);
    assert!(json.get("pagination").is_some());
}

#[sqlx::test]
async fn test_stats_list_with_clicks(pool: PgPool) {
    let domain_id = common::create_test_domain(&pool, "clicks-list.com").await;
    common::create_test_link(&pool, "popular", "https://example.com", domain_id).await;

    let link_id: i64 = sqlx::query_scalar!("SELECT id FROM links WHERE code = 'popular'")
        .fetch_one(&pool)
        .await
        .unwrap();

    let reader = FakeStatsReader::new().with_link(link_id, canned_clicks(link_id, 10));
    let state = common::create_test_state_with(
        pool.clone(),
        Arc::new(NoopClickPublisher),
        Arc::new(reader),
    );
    let app = Router::new()
        .route("/api/stats", get(stats_list_handler))
        .with_state(state);

    let server = TestServer::new(app).unwrap();

    let response = server.get("/api/stats").await;

    response.assert_status_ok();

    let json = response.json::<serde_json::Value>();
    let items = json["items"].as_array().unwrap();

    let popular = items.iter().find(|item| item["code"] == "popular");

    assert!(popular.is_some());
    assert_eq!(popular.unwrap()["total"], 10);
}

#[sqlx::test]
async fn test_stats_list_pagination(pool: PgPool) {
    let domain_id = common::create_test_domain(&pool, "many-links.com").await;

    for i in 1..=30 {
        common::create_test_link(
            &pool,
            &format!("many{}", i),
            &format!("https://example.com/{}", i),
            domain_id,
        )
        .await;
    }

    let state = common::create_test_state_with(
        pool.clone(),
        Arc::new(NoopClickPublisher),
        Arc::new(FakeStatsReader::new()),
    );
    let app = Router::new()
        .route("/api/stats", get(stats_list_handler))
        .with_state(state);

    let server = TestServer::new(app).unwrap();

    let response = server
        .get("/api/stats")
        .add_query_param("page", "2")
        .add_query_param("page_size", "10")
        .await;

    response.assert_status_ok();

    let json = response.json::<serde_json::Value>();
    assert_eq!(json["pagination"]["page"], 2);
    assert_eq!(json["pagination"]["page_size"], 10);
    assert!(json["pagination"]["total_items"].as_i64().unwrap() >= 30);
}
