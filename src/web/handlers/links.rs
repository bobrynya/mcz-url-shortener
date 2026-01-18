use askama::Template;
use askama_web::WebTemplate;
use axum::response::IntoResponse;

#[derive(Template, WebTemplate)]
#[template(path = "links.html")]
pub struct LinksTemplate {}

pub async fn links_handler() -> impl IntoResponse {
    LinksTemplate {}
}
