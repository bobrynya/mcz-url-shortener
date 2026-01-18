use askama::Template;
use askama_web::WebTemplate;
use axum::{extract::Path, response::IntoResponse};

#[derive(Template, WebTemplate)]
#[template(path = "stats.html")]
pub struct StatsTemplate {
    pub code: String,
}

pub async fn stats_handler(Path(code): Path<String>) -> impl IntoResponse {
    StatsTemplate { code }
}
