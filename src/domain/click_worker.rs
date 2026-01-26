use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_retry::RetryIf;
use tokio_retry::strategy::ExponentialBackoff;

use crate::domain::click_event::ClickEvent;
use crate::domain::entities::NewClick;
use crate::domain::repositories::{DomainRepository, LinkRepository, StatsRepository};
use crate::error::AppError;

/// Проверка, является ли ошибка временной
fn is_transient_error(e: &AppError) -> bool {
    matches!(e, AppError::Internal { .. })
}

/// Воркер для асинхронной обработки кликов
pub async fn run_click_worker<S, D, L>(
    mut rx: mpsc::Receiver<ClickEvent>,
    stats_repository: Arc<S>,
    domain_repository: Arc<D>,
    link_repository: Arc<L>,
) where
    S: StatsRepository,
    D: DomainRepository,
    L: LinkRepository,
{
    tracing::info!("Click worker started");

    while let Some(ev) = rx.recv().await {
        metrics::counter!("click_worker_received_total").increment(1);

        // Стратегия повторов: 100ms, 200ms, 400ms, 800ms, 1.6s, 3.2s
        let strategy = ExponentialBackoff::from_millis(100).take(6);

        let stats_repo = stats_repository.clone();
        let domain_repo = domain_repository.clone();
        let link_repo = link_repository.clone();
        let event = ev.clone();

        let op = || {
            let stats_repo = stats_repo.clone();
            let domain_repo = domain_repo.clone();
            let link_repo = link_repo.clone();
            let event = event.clone();

            async move {
                // 1. Получаем domain_id по domain
                let domain_entity = domain_repo.find_by_name(&event.domain).await?;

                // 2. Получаем link_id по code и domain_id
                let link = link_repo
                    .find_by_code(&event.code, domain_entity.unwrap().id)
                    .await?;

                // 3. Преобразуем ClickEvent в NewClick
                let new_click = NewClick {
                    link_id: link.unwrap().id,
                    user_agent: event.user_agent,
                    referer: event.referer,
                    ip: event.ip,
                };

                // 4. Записываем через репозиторий
                stats_repo.record_click(new_click).await.map(|_| ())
            }
        };

        let on_error = |e: &AppError| {
            let transient = is_transient_error(e);
            if transient {
                metrics::counter!("click_worker_retried_total").increment(1);
                tracing::warn!(
                    domain = &event.domain,
                    code = &event.code,
                    error = ?e,
                    "Click worker: transient error, retrying"
                );
            }
            transient
        };

        let res = RetryIf::spawn(strategy, op, on_error).await;

        match res {
            Ok(()) => {
                metrics::counter!("click_worker_processed_total").increment(1);
                tracing::debug!(
                    domain = &event.domain,
                    code = &event.code,
                    "Click successfully recorded"
                );
            }
            Err(e) => {
                // Нетранзиентная ошибка или исчерпаны все попытки
                metrics::counter!("click_worker_failed_total").increment(1);
                metrics::counter!("click_worker_dropped_total").increment(1);

                tracing::error!(
                    error = ?e,
                    domain = &event.domain,
                    code = &event.code,
                    "Click worker: failed to persist click event after retries"
                );
            }
        }
    }

    tracing::info!("Click worker stopped");
}
