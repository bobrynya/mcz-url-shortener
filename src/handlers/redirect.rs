use axum::{
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, header},
    response::Redirect,
};
use chrono::Utc;
use serde_json::json;
use std::net::IpAddr;
use std::net::SocketAddr;

use crate::{
    domain::click_event::ClickEvent,
    error::{AppError, map_sqlx_error},
    state::AppState,
};

pub async fn redirect_by_code(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(st): State<AppState>,
    Path(code): Path<String>,
) -> Result<Redirect, AppError> {
    let rec = sqlx::query!(
        r#"
        UPDATE links
        SET clicks = clicks + 1
        WHERE code = $1
        RETURNING id, long_url
        "#,
        code
    )
    .fetch_optional(&st.db)
    .await
    .map_err(map_sqlx_error)?;

    let r = rec.ok_or_else(|| AppError::not_found("Unknown code", json!({ "code": code })))?;

    let referer = headers
        .get(header::REFERER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let ip: IpAddr = addr.ip();

    if let Err(e) = st.click_tx.try_send(ClickEvent {
        link_id: r.id,
        clicked_at: Utc::now(),
        referer,
        user_agent,
        ip: Some(ip),
    }) {
        // очередь переполнена или воркер умер — редирект всё равно отдаём
        tracing::warn!(error = %e, "failed to enqueue click event");
    }

    Ok(Redirect::temporary(&r.long_url))
}
