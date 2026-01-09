use axum::{
    extract::{Query, State},
    Json,
};
use serde_json::json;

use crate::{
    dto::{
        stats::StatsResponse,
        stats_list::{StatsListQuery, StatsListResponse},
    },
    error::{map_sqlx_error, AppError},
    state::AppState,
};

pub async fn stats_list(
    State(st): State<AppState>,
    Query(q): Query<StatsListQuery>,
) -> Result<Json<StatsListResponse>, AppError> {
    let page = q.page.unwrap_or(1);
    if page == 0 {
        return Err(AppError::bad_request(
            "page must be >= 1",
            json!({"field": "page", "min": 1}),
        ));
    }

    let page_size = q.page_size.unwrap_or(25);
    if !(10..=50).contains(&page_size) {
        return Err(AppError::bad_request(
            "page_size must be in [10..50]",
            json!({"field": "page_size", "min": 10, "max": 50}),
        ));
    }

    let from = q.from;
    let to = q.to;

    if let (Some(f), Some(t)) = (from, to) {
        if f >= t {
            return Err(AppError::bad_request(
                "from must be < to",
                json!({"field": "from/to"}),
            ));
        }
    }

    let limit: i64 = page_size as i64;
    let offset: i64 = ((page - 1) as i64) * limit;

    let total_row = sqlx::query!(
        r#"
    SELECT COUNT(*)::bigint AS "total!"
    FROM links l
    WHERE
      ($1::timestamptz IS NULL AND $2::timestamptz IS NULL)
      OR EXISTS (
        SELECT 1
        FROM link_clicks lc
        WHERE lc.link_id = l.id
          AND ($1::timestamptz IS NULL OR lc.clicked_at >= $1)
          AND ($2::timestamptz IS NULL OR lc.clicked_at <  $2)
      )
    "#,
        from,
        to,
    )
    .fetch_one(&st.db)
    .await
    .map_err(map_sqlx_error)?;
    let total = total_row.total;

    let rows = sqlx::query!(
        r#"
    SELECT l.long_url, l.code, l.clicks as "clicks!", l.created_at as "created_at!"
    FROM links l
    WHERE
      ($1::timestamptz IS NULL AND $2::timestamptz IS NULL)
      OR EXISTS (
        SELECT 1
        FROM link_clicks lc
        WHERE lc.link_id = l.id
          AND ($1::timestamptz IS NULL OR lc.clicked_at >= $1)
          AND ($2::timestamptz IS NULL OR lc.clicked_at <  $2)
      )
    ORDER BY l.created_at DESC
    LIMIT $3 OFFSET $4
    "#,
        from,
        to,
        limit,
        offset
    )
    .fetch_all(&st.db)
    .await
    .map_err(map_sqlx_error)?;

    let items = rows
        .into_iter()
        .map(|r| StatsResponse {
            long_url: r.long_url,
            code: r.code,
            clicks: r.clicks,
            created_at: r.created_at,
        })
        .collect();

    Ok(Json(StatsListResponse {
        page,
        page_size,
        total,
        items,
    }))
}
