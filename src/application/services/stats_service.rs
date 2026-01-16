use std::sync::Arc;

use crate::domain::entities::{Click, NewClick};
use crate::domain::repositories::{DetailedStats, LinkStats, StatsFilter, StatsRepository};
use crate::error::AppError;
use serde_json::json;

pub struct StatsService<R: StatsRepository> {
    repository: Arc<R>,
}

impl<R: StatsRepository> StatsService<R> {
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }

    #[allow(dead_code)]
    pub async fn record_click(
        &self,
        link_id: i64,
        user_agent: Option<String>,
        referer: Option<String>,
        ip: Option<String>,
    ) -> Result<Click, AppError> {
        let new_click = NewClick {
            link_id,
            user_agent,
            referer,
            ip,
        };

        self.repository.record_click(new_click).await
    }

    pub async fn get_detailed_stats(
        &self,
        code: &str,
        filter: StatsFilter,
    ) -> Result<DetailedStats, AppError> {
        self.repository
            .get_stats_by_code(code, filter)
            .await?
            .ok_or_else(|| AppError::not_found("Statistics not found", json!({ "code": code })))
    }

    pub async fn get_all_stats(&self, filter: StatsFilter) -> Result<Vec<LinkStats>, AppError> {
        self.repository.get_all_stats(filter).await
    }

    pub async fn count_all_links(&self) -> Result<i64, AppError> {
        self.repository.count_all_links().await
    }
}
