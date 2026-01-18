use askama::Template;
use askama_web::WebTemplate;
use axum::response::IntoResponse;

#[derive(Template, WebTemplate)]
#[template(path = "dashboard.html")]
pub struct DashboardTemplate {}

pub async fn dashboard_handler() -> impl IntoResponse {
    DashboardTemplate {}
}
