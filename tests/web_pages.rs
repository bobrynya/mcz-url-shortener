mod common;

use axum::Router;
use axum_test::TestServer;
use sqlx::PgPool;
use url_shortener::web::routes::public_routes;

/// Renders the login page through the real dashboard router. This guards the
/// Askama template pipeline (template inheritance `login.html` → `base.html`)
/// against breakage on template-engine upgrades.
#[sqlx::test]
async fn test_login_page_renders(pool: PgPool) {
    let (state, _rx) = common::create_test_state(pool);
    let app = Router::new()
        .nest("/dashboard", public_routes())
        .with_state(state);
    let server = TestServer::new(app).unwrap();

    let response = server.get("/dashboard/login").await;

    response.assert_status_ok();
    let body = response.text();
    assert!(
        body.contains("<title>Login - URL Shortener</title>"),
        "rendered page is missing the inherited <title> from base.html"
    );
    assert!(
        body.contains("Dashboard Login"),
        "rendered page is missing the login heading"
    );
    assert!(
        body.contains("loginPage()"),
        "rendered page is missing the Alpine.js hook"
    );
}
