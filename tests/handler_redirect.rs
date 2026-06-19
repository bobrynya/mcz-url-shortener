mod common;

use axum::http::StatusCode;
use axum::{Router, extract::ConnectInfo, routing::get};
use axum_test::TestServer;
use sqlx::PgPool;
use std::net::SocketAddr;
use tower::Layer;
use url_shortener::api::handlers::redirect_handler;

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
    let (state, _rx) = common::create_test_state(pool.clone());
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
    let (state, _rx) = common::create_test_state(pool);
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

#[sqlx::test]
async fn test_redirect_records_click(pool: PgPool) {
    let (state, mut rx) = common::create_test_state(pool.clone());
    let app = Router::new()
        .route("/{code}", get(redirect_handler))
        .layer(MockConnectInfoLayer)
        .with_state(state);

    let server = TestServer::new(app).unwrap();

    let domain_id = common::get_default_domain(&pool).await;
    common::create_test_link(&pool, "clickme", "https://example.com", domain_id).await;

    let response = server
        .get("/clickme")
        .add_header("Host", "s.example.com")
        .add_header("User-Agent", "TestBot/1.0")
        .await;

    assert_eq!(response.status_code(), 307);

    let click_event = rx.try_recv();
    assert!(click_event.is_ok());
    assert_eq!(click_event.unwrap().code, "clickme");
}

#[sqlx::test]
async fn test_redirect_with_user_agent_and_referer(pool: PgPool) {
    let (state, mut rx) = common::create_test_state(pool.clone());
    let app = Router::new()
        .route("/{code}", get(redirect_handler))
        .layer(MockConnectInfoLayer)
        .with_state(state);

    let server = TestServer::new(app).unwrap();

    let domain_id = common::get_default_domain(&pool).await;
    common::create_test_link(&pool, "track", "https://example.com", domain_id).await;

    let response = server
        .get("/track")
        .add_header("Host", "s.example.com")
        .add_header("User-Agent", "Mozilla/5.0")
        .add_header("Referer", "https://google.com")
        .await;

    assert_eq!(response.status_code(), 307);

    let click_event = rx.try_recv();
    assert!(click_event.is_ok());
    let event = click_event.unwrap();
    assert_eq!(event.code, "track");
    assert_eq!(event.user_agent, Some("Mozilla/5.0".to_string()));
    assert_eq!(event.referer, Some("https://google.com".to_string()));
}

#[sqlx::test]
async fn test_redirect_missing_host_header(pool: PgPool) {
    let (state, _rx) = common::create_test_state(pool);
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
    let (state, _rx) = common::create_test_state(pool);
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
