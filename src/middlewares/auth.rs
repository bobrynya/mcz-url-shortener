use axum::{
    extract::{FromRequestParts, Request, State},
    middleware::Next,
    response::Response,
};
use axum_auth::AuthBearer;
use sha2::{Digest, Sha256};

use crate::{error::AppError, state::AppState};

fn hash_token(token: &str) -> String {
    let mut h = Sha256::new();
    h.update(token.as_bytes());
    hex::encode(h.finalize())
}

pub async fn auth_mw(
    State(st): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, AppError> {
    let (mut parts, body) = req.into_parts();

    let AuthBearer(token) = AuthBearer::from_request_parts(&mut parts, &())
        .await
        .map_err(|_| {
            AppError::unauthorized(
                "Unauthorized",
                serde_json::json!({"reason": "Authorization header is missing or invalid"}),
            )
        })?;

    // собираем Request обратно
    let req = Request::from_parts(parts, body);

    // validate token по БД
    let token_hash = hash_token(&token);
    let row = sqlx::query!(
        r#"
        SELECT id
        FROM api_tokens
        WHERE token_hash = $1
          AND revoked_at IS NULL
        "#,
        token_hash
    )
    .fetch_optional(&st.db)
    .await
    .map_err(crate::error::map_sqlx_error)?;

    if row.is_none() {
        return Err(AppError::unauthorized(
            "Unauthorized",
            serde_json::json!({"reason": "Invalid token"}),
        ));
    }

    Ok(next.run(req).await)
}
