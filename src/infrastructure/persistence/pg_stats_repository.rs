use async_trait::async_trait;
use sqlx::PgPool;
use std::sync::Arc;

use crate::domain::entities::{Click, Link, NewClick};
use crate::domain::repositories::{DetailedStats, LinkStats, StatsFilter, StatsRepository};
use crate::error::AppError;

/// PostgreSQL реализация репозитория статистики
pub struct PgStatsRepository {
    pool: Arc<PgPool>,
}

impl PgStatsRepository {
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl StatsRepository for PgStatsRepository {
    async fn record_click(&self, new_click: NewClick) -> Result<Click, AppError> {
        let row = sqlx::query!(
            r#"
            INSERT INTO link_clicks (link_id, user_agent, referer, ip)
            VALUES ($1, $2, $3, $4)
            RETURNING id, link_id, clicked_at, user_agent, referer, ip
            "#,
            new_click.link_id,
            new_click.user_agent,
            new_click.referer,
            new_click.ip
        )
        .fetch_one(self.pool.as_ref())
        .await?;

        Ok(Click::new(
            row.id,
            row.link_id,
            row.clicked_at,
            row.user_agent,
            row.referer,
            row.ip,
        ))
    }

    async fn get_stats_by_code(
        &self,
        code: &str,
        filter: StatsFilter,
    ) -> Result<Option<DetailedStats>, AppError> {
        let link_row = sqlx::query!(
            r#"
            SELECT l.id, l.code, l.long_url, d.domain as "domain?", l.created_at
            FROM links l
            LEFT JOIN domains d ON d.id = l.domain_id
            WHERE code = $1 AND ($2::bigint IS NULL OR domain_id = $2)
            "#,
            code,
            filter.domain_id,
        )
        .fetch_optional(self.pool.as_ref())
        .await?;

        let link_row = match link_row {
            Some(row) => row,
            None => return Ok(None),
        };

        let link = Link::new(
            link_row.id,
            link_row.code,
            link_row.long_url,
            link_row.domain,
            link_row.created_at,
        );

        // Подсчитываем общее количество кликов с учётом фильтров по дате
        let total = self
            .count_clicks_by_link_id(link.id, filter.from_date, filter.to_date)
            .await?;

        // Получаем клики с фильтрами и пагинацией
        let click_rows = sqlx::query!(
            r#"
            SELECT id, link_id, clicked_at, user_agent, referer, ip
            FROM link_clicks
            WHERE link_id = $1
              AND ($2::timestamptz IS NULL OR clicked_at >= $2)
              AND ($3::timestamptz IS NULL OR clicked_at <= $3)
            ORDER BY clicked_at DESC
            LIMIT $4 OFFSET $5
            "#,
            link.id,
            filter.from_date,
            filter.to_date,
            filter.limit,
            filter.offset
        )
        .fetch_all(self.pool.as_ref())
        .await?;

        let items = click_rows
            .into_iter()
            .map(|r| Click::new(r.id, r.link_id, r.clicked_at, r.user_agent, r.referer, r.ip))
            .collect();

        Ok(Some(DetailedStats { link, total, items }))
    }

    async fn get_all_stats(&self, filter: StatsFilter) -> Result<Vec<LinkStats>, AppError> {
        let rows = sqlx::query!(
            r#"
            SELECT
                l.id,
                l.code,
                l.long_url,
                l.created_at,
                d.domain as "domain?",
                COUNT(lc.id) as "clicks!"
            FROM links l
            LEFT JOIN link_clicks lc ON l.id = lc.link_id
                AND ($1::timestamptz IS NULL OR lc.clicked_at >= $1)
                AND ($2::timestamptz IS NULL OR lc.clicked_at <= $2)
            LEFT JOIN domains d ON d.id = l.domain_id
            WHERE ($5::bigint IS NULL OR l.domain_id = $5)
            GROUP BY l.id, l.code, l.long_url, l.created_at, d.domain
            ORDER BY l.created_at DESC
            LIMIT $3 OFFSET $4
            "#,
            filter.from_date,
            filter.to_date,
            filter.limit,
            filter.offset,
            filter.domain_id,
        )
        .fetch_all(self.pool.as_ref())
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| LinkStats {
                link_id: r.id,
                code: r.code,
                domain: r.domain,
                long_url: r.long_url,
                total: r.clicks,
                created_at: r.created_at,
            })
            .collect())
    }

    async fn count_all_links(&self) -> Result<i64, AppError> {
        let row = sqlx::query!(
            r#"
            SELECT COUNT(*) as count
            FROM links
            "#
        )
        .fetch_one(self.pool.as_ref())
        .await?;

        Ok(row.count.unwrap_or(0))
    }

    async fn count_clicks_by_link_id(
        &self,
        link_id: i64,
        from_date: Option<chrono::DateTime<chrono::Utc>>,
        to_date: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<i64, AppError> {
        let row = sqlx::query!(
            r#"
            SELECT COUNT(*) as count
            FROM link_clicks
            WHERE link_id = $1
              AND ($2::timestamptz IS NULL OR clicked_at >= $2)
              AND ($3::timestamptz IS NULL OR clicked_at <= $3)
            "#,
            link_id,
            from_date,
            to_date
        )
        .fetch_one(self.pool.as_ref())
        .await?;

        Ok(row.count.unwrap_or(0))
    }
}
