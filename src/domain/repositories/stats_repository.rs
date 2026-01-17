use crate::domain::entities::{Click, NewClick};
use crate::error::AppError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct LinkStats {
    #[allow(dead_code)]
    pub link_id: i64,
    pub code: String,
    pub domain: Option<String>,
    pub long_url: String,
    pub total: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct DetailedStats {
    pub link: crate::domain::entities::Link,
    pub total: i64,
    pub items: Vec<Click>,
}

#[derive(Debug, Clone)]
pub struct StatsFilter {
    pub from_date: Option<DateTime<Utc>>,
    pub to_date: Option<DateTime<Utc>>,
    pub offset: i64,
    pub limit: i64,
    pub domain_id: Option<i64>,
}

impl StatsFilter {
    pub fn new(offset: i64, limit: i64) -> Self {
        Self {
            from_date: None,
            to_date: None,
            offset,
            limit,
            domain_id: None,
        }
    }

    pub fn with_domain(mut self, domain_id: Option<i64>) -> Self {
        self.domain_id = domain_id;
        self
    }

    pub fn with_date_range(
        mut self,
        from_date: Option<DateTime<Utc>>,
        to_date: Option<DateTime<Utc>>,
    ) -> Self {
        self.from_date = from_date;
        self.to_date = to_date;
        self
    }
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
