use crate::domain::entities::{Click, NewClick};
use crate::error::AppError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct LinkStats {
    #[allow(dead_code)]
    pub link_id: i64,
    pub code: String,
    pub long_url: String,
    pub total_clicks: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct DetailedStats {
    pub link: crate::domain::entities::Link,
    pub total_clicks: i64,
    pub recent_clicks: Vec<Click>,
}

#[derive(Debug, Clone)]
pub struct StatsFilter {
    pub from_date: Option<DateTime<Utc>>,
    pub to_date: Option<DateTime<Utc>>,
    pub offset: i64,
    pub limit: i64,
}

/// Репозиторий для работы со статистикой и кликами
#[async_trait]
pub trait StatsRepository: Send + Sync {
    /// Записать клик
    async fn record_click(&self, new_click: NewClick) -> Result<Click, AppError>;

    /// Получить статистику по конкретному коду (с фильтрами)
    async fn get_stats_by_code(
        &self,
        code: &str,
        filter: StatsFilter,
    ) -> Result<Option<DetailedStats>, AppError>;

    /// Получить список всех ссылок со статистикой (с фильтрами)
    async fn get_all_stats(&self, filter: StatsFilter) -> Result<Vec<LinkStats>, AppError>;

    /// Подсчитать общее количество ссылок (для пагинации)
    async fn count_all_links(&self) -> Result<i64, AppError>;

    /// Получить количество кликов по link_id (с фильтром по дате)
    async fn count_clicks_by_link_id(
        &self,
        link_id: i64,
        from_date: Option<DateTime<Utc>>,
        to_date: Option<DateTime<Utc>>,
    ) -> Result<i64, AppError>;
}
