use askama::Template;
use askama_web::WebTemplate;
use axum::response::IntoResponse;

// Шаблон для страницы логина
#[derive(Template, WebTemplate)]
#[template(path = "login.html")]
struct LoginTemplate {}

pub async fn login_handler() -> impl IntoResponse {
    LoginTemplate {}
}
