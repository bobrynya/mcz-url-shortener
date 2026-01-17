use axum::{
    extract::{FromRequestParts, Request, State},
    middleware::Next,
    response::Response,
};
use axum_auth::AuthBearer;

use crate::{error::AppError, state::AppState};

pub async fn layer(
    State(st): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, AppError> {
    let (mut parts, body) = req.into_parts();

    // Извлекаем Bearer токен из заголовка Authorization
    let AuthBearer(token) = AuthBearer::from_request_parts(&mut parts, &())
        .await
        .map_err(|_| {
            AppError::unauthorized(
                "Unauthorized",
                serde_json::json!({"reason": "Authorization header is missing or invalid"}),
            )
        })?;

    // Собираем Request обратно
    let req = Request::from_parts(parts, body);

    // Аутентифицируем через сервис
    st.auth_service.authenticate(&token).await?;

    Ok(next.run(req).await)
}
