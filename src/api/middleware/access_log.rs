use axum::{
    extract::{ConnectInfo, Request},
    http::header,
    middleware::Next,
    response::Response,
};
use std::{net::SocketAddr, time::Instant};

pub async fn access_log_mw(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    let start = Instant::now();

    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    let version = format!("{:?}", req.version());

    let ua = req
        .headers()
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-")
        .to_string();

    let referer = req
        .headers()
        .get(header::REFERER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-")
        .to_string();

    let response = next.run(req).await;

    let status = response.status().as_u16();
    let ms = start.elapsed().as_millis();

    tracing::info!(
        r#"{ip} - - "{method} {path} {version}" {status} - "{referer}" "{ua}" {ms}ms"#,
        ip = addr.ip(),
        method = method,
        path = path,
        version = version,
        status = status,
        referer = referer,
        ua = ua,
        ms = ms,
    );

    response
}
