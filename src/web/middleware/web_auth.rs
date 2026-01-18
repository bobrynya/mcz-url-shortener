use axum::{
    extract::{Request, State},
    http::header::COOKIE,
    middleware::Next,
    response::{Redirect, Response},
};

use crate::state::AppState;

pub async fn layer(
    State(st): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, Redirect> {
    // Извлекаем cookie из заголовка
    let token = req
        .headers()
        .get(COOKIE)
        .and_then(|cookie_header| cookie_header.to_str().ok())
        .and_then(|cookie_str| {
            // Парсим cookie строку
            cookie_str.split(';').find_map(|cookie| {
                let mut parts = cookie.trim().splitn(2, '=');
                match (parts.next(), parts.next()) {
                    (Some("mcz_token"), Some(value)) => Some(value.to_string()),
                    _ => None,
                }
            })
        });

    match token {
        Some(token) => {
            // Проверяем токен через auth_service
            match st.auth_service.authenticate(&token).await {
                Ok(_) => {
                    // Токен валиден - пропускаем запрос
                    Ok(next.run(req).await)
                }
                Err(_) => {
                    // Токен невалиден - редирект на login
                    Err(Redirect::to("/dashboard/login"))
                }
            }
        }
        None => {
            // Нет токена - редирект на login
            Err(Redirect::to("/dashboard/login"))
        }
    }
}
