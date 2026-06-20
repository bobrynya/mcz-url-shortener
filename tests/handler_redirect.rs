mod common;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::http::StatusCode;
use axum::{Router, extract::ConnectInfo, routing::get};
use axum_test::TestServer;
use sqlx::PgPool;
use tower::Layer;
use url_shortener::api::handlers::redirect_handler;
use url_shortener::infrastructure::persistence::UnavailableStatsReader;

use common::RecordingClickPublisher;

#[derive(Clone)]
struct MockConnectInfoLayer;

impl<S> Layer<S> for MockConnectInfoLayer {
    type Service = MockConnectInfoService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        MockConnectInfoService { inner }
    }
}

#[derive(Clone)]
struct MockConnectInfoService<S> {
    inner: S,
}

impl<S, B> tower::Service<axum::http::Request<B>> for MockConnectInfoService<S>
where
    S: tower::Service<axum::http::Request<B>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    B: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: axum::http::Request<B>) -> Self::Future {
        let addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();
        req.extensions_mut().insert(ConnectInfo(addr));
        self.inner.call(req)
    }
}

#[sqlx::test]
async fn test_redirect_success(pool: PgPool) {
    let state = common::create_test_state(pool.clone());
    let app = Router::new()
        .route("/{code}", get(redirect_handler))
        .layer(MockConnectInfoLayer)
        .with_state(state);

    let server = TestServer::new(app).unwrap();

    let domain_id = common::get_default_domain(&pool).await;
    common::create_test_link(&pool, "redirect1", "https://example.com/target", domain_id).await;

    let response = server
        .get("/redirect1")
        .add_header("Host", "s.example.com")
        .await;

    assert_eq!(response.status_code(), 307);

    let location = response.header("location");
    assert_eq!(location, "https://example.com/target");
}

#[sqlx::test]
async fn test_redirect_not_found(pool: PgPool) {
    let state = common::create_test_state(pool);
    let app = Router::new()
        .route("/{code}", get(redirect_handler))
        .layer(MockConnectInfoLayer)
        .with_state(state);

    let server = TestServer::new(app).unwrap();

    let response = server
        .get("/notfound")
        .add_header("Host", "s.example.com")
        .await;

    response.assert_status_not_found();
}

/// Polls the recording publisher until it has captured at least one event.
///
/// The redirect handler publishes the click via `tokio::spawn` (fire-and-forget),
/// so the event may not be recorded by the time the HTTP response returns.
async fn wait_for_click(
    publisher: &RecordingClickPublisher,
) -> url_shortener::domain::click_event::ClickEvent {
    for _ in 0..100 {
        if let Some(event) = publisher.events().into_iter().next() {
            return event;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("no click event was published within the timeout");
}

#[sqlx::test]
async fn test_redirect_records_click(pool: PgPool) {
    let publisher = RecordingClickPublisher::new();
    let state = common::create_test_state_with(
        pool.clone(),
        Arc::new(publisher.clone()),
        Arc::new(UnavailableStatsReader),
    );
    let app = Router::new()
        .route("/{code}", get(redirect_handler))
        .layer(MockConnectInfoLayer)
        .with_state(state);

    let server = TestServer::new(app).unwrap();

    let domain_id = common::get_default_domain(&pool).await;
    common::create_test_link(&pool, "clickme", "https://example.com", domain_id).await;

    let link_id: i64 = sqlx::query_scalar!("SELECT id FROM links WHERE code = 'clickme'")
        .fetch_one(&pool)
        .await
        .unwrap();

    let response = server
        .get("/clickme")
        .add_header("Host", "s.example.com")
        .add_header("User-Agent", "TestBot/1.0")
        .await;

    assert_eq!(response.status_code(), 307);

    let event = wait_for_click(&publisher).await;
    assert_eq!(event.link_id, link_id);
}

#[sqlx::test]
async fn test_redirect_with_user_agent_and_referer(pool: PgPool) {
    let publisher = RecordingClickPublisher::new();
    let state = common::create_test_state_with(
        pool.clone(),
        Arc::new(publisher.clone()),
        Arc::new(UnavailableStatsReader),
    );
    let app = Router::new()
        .route("/{code}", get(redirect_handler))
        .layer(MockConnectInfoLayer)
        .with_state(state);

    let server = TestServer::new(app).unwrap();

    let domain_id = common::get_default_domain(&pool).await;
    common::create_test_link(&pool, "track", "https://example.com", domain_id).await;

    let link_id: i64 = sqlx::query_scalar!("SELECT id FROM links WHERE code = 'track'")
        .fetch_one(&pool)
        .await
        .unwrap();

    let response = server
        .get("/track")
        .add_header("Host", "s.example.com")
        .add_header("User-Agent", "Mozilla/5.0")
        .add_header("Referer", "https://google.com")
        .await;

    assert_eq!(response.status_code(), 307);

    let event = wait_for_click(&publisher).await;
    assert_eq!(event.link_id, link_id);
    assert_eq!(event.user_agent, Some("Mozilla/5.0".to_string()));
    assert_eq!(event.referer, Some("https://google.com".to_string()));
}

#[sqlx::test]
async fn test_redirect_missing_host_header(pool: PgPool) {
    let state = common::create_test_state(pool);
    let app = Router::new()
        .route("/{code}", get(redirect_handler))
        .layer(MockConnectInfoLayer)
        .with_state(state);

    let server = TestServer::new(app).unwrap();

    let response = server.get("/anycode").await;

    response.assert_status_bad_request();
}

fn make_redirect_server(pool: PgPool) -> TestServer {
    use url_shortener::api::handlers::redirect_handler;
    let state = common::create_test_state(pool);
    let app = Router::new()
        .route("/{code}", get(redirect_handler))
        .layer(MockConnectInfoLayer)
        .with_state(state);
    TestServer::new(app).unwrap()
}

#[sqlx::test]
async fn test_redirect_deleted_link_returns_410(pool: PgPool) {
    let domain_id = common::get_default_domain(&pool).await;
    common::create_deleted_link(&pool, "gone1", "https://example.com", domain_id).await;

    let server = make_redirect_server(pool);
    let response = server
        .get("/gone1")
        .add_header("Host", "s.example.com")
        .await;

    assert_eq!(response.status_code(), StatusCode::GONE);

    let body = response.json::<serde_json::Value>();
    assert_eq!(body["error"]["code"], "gone");
}

#[sqlx::test]
async fn test_redirect_expired_link_returns_410(pool: PgPool) {
    let domain_id = common::get_default_domain(&pool).await;
    common::create_expired_link(&pool, "gone2", "https://example.com", domain_id).await;

    let server = make_redirect_server(pool);
    let response = server
        .get("/gone2")
        .add_header("Host", "s.example.com")
        .await;

    assert_eq!(response.status_code(), StatusCode::GONE);

    let body = response.json::<serde_json::Value>();
    assert_eq!(body["error"]["code"], "gone");
}

#[sqlx::test]
async fn redirect_emits_cache_miss_metric(pool: PgPool) {
    use url_shortener::observability::metrics::install_prometheus_recorder;

    let handle = install_prometheus_recorder();
    let domain_id = common::get_default_domain(&pool).await;
    common::create_test_link(&pool, "cmiss", "https://example.com/cmiss", domain_id).await;

    let state = common::create_test_state(pool.clone());
    let app = Router::new()
        .route("/{code}", get(redirect_handler))
        .layer(MockConnectInfoLayer)
        .with_state(state);
    let server = TestServer::new(app).unwrap();

    // NullCache always misses, so the first redirect emits result="miss".
    server
        .get("/cmiss")
        .add_header("Host", "s.example.com")
        .await;

    let body = handle.render();
    assert!(
        body.contains("cache_requests_total"),
        "missing cache_requests_total in:\n{body}"
    );
    assert!(
        body.contains(r#"result="miss""#),
        "missing result=\"miss\" label in:\n{body}"
    );
}

/// axum's `Redirect::permanent` issues 308 Permanent Redirect (method-preserving),
/// not 301 Moved Permanently. Both are permanent; 308 is the modern standard.
#[sqlx::test]
async fn test_redirect_permanent_link_returns_308(pool: PgPool) {
    let domain_id = common::get_default_domain(&pool).await;
    common::create_permanent_link(&pool, "perm1", "https://example.com/dest", domain_id).await;

    let server = make_redirect_server(pool);
    let response = server
        .get("/perm1")
        .add_header("Host", "s.example.com")
        .await;

    assert_eq!(response.status_code(), StatusCode::PERMANENT_REDIRECT); // 308

    let location = response.header("location");
    assert_eq!(location, "https://example.com/dest");
}
