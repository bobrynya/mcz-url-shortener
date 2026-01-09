use std::collections::{HashMap, HashSet};

use crate::{
    dto::shorten::{Item, ShortenInput, ShortenResponse},
    error::AppError,
    state::AppState,
    utils::{codegen::gen_code, db_error::is_unique_violation_on_code, url_norm::normalize_url},
};

use axum::{Json, extract::State};
use serde_json::json;

pub async fn shorten(
    State(st): State<AppState>,
    Json(payload): Json<ShortenInput>,
) -> Result<Json<ShortenResponse>, AppError> {
    // 0) Нормализуем вход в Vec
    let raw_urls: Vec<String> = match payload {
        ShortenInput::One(s) => vec![s],
        ShortenInput::Many(v) => v,
    };

    if raw_urls.is_empty() {
        return Err(AppError::bad_request(
            "Empty input",
            json!({"field": "urls"}),
        ));
    }

    // 1) Нормализация + валидация
    let mut urls: Vec<String> = Vec::with_capacity(raw_urls.len());
    for u in &raw_urls {
        let n = normalize_url(u)
            .map_err(|msg| AppError::bad_request(msg, json!({"field": "url", "value": u})))?;
        urls.push(n);
    }

    // 2) Дедуп (по нормализованной строке), порядок сохраняем
    let mut seen: HashSet<String> = HashSet::new();
    urls.retain(|u| seen.insert(u.clone()));

    if urls.is_empty() {
        return Err(AppError::bad_request(
            "No valid URLs after normalization",
            json!({"field": "urls"}),
        ));
    }

    // 3) SELECT существующих
    let existing_rows = sqlx::query!(
        r#"
        SELECT long_url, code
        FROM links
        WHERE long_url = ANY($1::text[])
        "#,
        &urls[..]
    )
    .fetch_all(&st.db)
    .await
    .map_err(|e| {
        AppError::internal(
            "Database error",
            json!({"op": "select_existing", "cause": e.to_string()}),
        )
    })?;

    let mut existing: HashMap<String, String> = existing_rows
        .into_iter()
        .map(|r| (r.long_url, r.code))
        .collect();

    // 4) Список новых (вставлять будем только их)
    let existing_set: HashSet<&String> = existing.keys().collect();
    let new_urls: Vec<String> = urls
        .iter()
        .filter(|u| !existing_set.contains(*u))
        .cloned()
        .collect();

    // 5) INSERT только новых, с retry на коллизию code
    let mut inserted_ok = false;

    for attempt in 0..5 {
        if new_urls.is_empty() {
            inserted_ok = true;
            break;
        }

        let new_codes: Vec<String> = new_urls.iter().map(|_| gen_code()).collect();

        let inserted = sqlx::query!(
            r#"
            WITH input AS (
              SELECT * FROM UNNEST($1::text[], $2::text[]) AS t(long_url, code)
            ),
            ins AS (
              INSERT INTO links (long_url, code)
              SELECT long_url, code FROM input
              ON CONFLICT (long_url) DO UPDATE
                SET long_url = EXCLUDED.long_url
              RETURNING long_url, code
            )
            SELECT long_url, code FROM ins
            "#,
            &new_urls[..],
            &new_codes[..],
        )
        .fetch_all(&st.db)
        .await;

        match inserted {
            Ok(rows) => {
                for r in rows {
                    existing.insert(r.long_url, r.code);
                }
                inserted_ok = true;
                break;
            }
            Err(e) => {
                // Только коллизия по links_code_key -> retry
                if is_unique_violation_on_code(&e) {
                    tracing::warn!(attempt, "code collision, retrying");
                    continue;
                }

                return Err(AppError::internal(
                    "Database error",
                    json!({"op": "insert_new", "cause": e.to_string()}),
                ));
            }
        }
    }

    if !inserted_ok {
        return Err(AppError::internal(
            "Failed to generate unique codes",
            json!({"op": "insert_new"}),
        ));
    }

    // 6) Ответ (в порядке уникальных urls)
    let items = urls
        .into_iter()
        .map(|long_url| {
            let code = existing
                .get(&long_url)
                .cloned()
                // Если вдруг не нашли — это уже внутренняя ошибка, но Item требует code.
                .unwrap_or_else(|| "ERROR_NO_CODE".to_string());

            let short_url = format!("{}/{}", st.base_url.trim_end_matches('/'), code);

            Item {
                long_url,
                code,
                short_url,
            }
        })
        .collect();

    Ok(Json(ShortenResponse { items }))
}
