use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_retry::RetryIf;
use tokio_retry::strategy::ExponentialBackoff;

use crate::domain::click_event::ClickEvent;
use crate::domain::entities::NewClick;
use crate::domain::repositories::StatsRepository;
use crate::error::AppError;

/// Проверка, является ли ошибка временной
fn is_transient_error(e: &AppError) -> bool {
    matches!(e, AppError::Internal { .. })
}

/// Воркер для асинхронной обработки кликов
pub async fn run_click_worker<R: StatsRepository>(
    mut rx: mpsc::Receiver<ClickEvent>,
    repository: Arc<R>,
) {
    tracing::info!("Click worker started");

    while let Some(ev) = rx.recv().await {
        metrics::counter!("click_worker_received_total").increment(1);

        // Стратегия повторов: 100ms, 200ms, 400ms, 800ms, 1.6s, 3.2s
        let strategy = ExponentialBackoff::from_millis(100).take(6);

        let repo = repository.clone();
        let event = ev.clone();

        let op = || {
            let repo = repo.clone();
            let event = event.clone();

            async move {
                // Преобразуем ClickEvent в NewClick
                let new_click = NewClick {
                    link_id: event.link_id,
                    user_agent: event.user_agent,
                    referer: event.referer,
                    ip: event.ip,
                };

                // Записываем через репозиторий
                repo.record_click(new_click).await.map(|_| ())
            }
        };

        let on_error = |e: &AppError| {
            let transient = is_transient_error(e);
            if transient {
                metrics::counter!("click_worker_retried_total").increment(1);
                tracing::warn!(
                    link_id = event.link_id,
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
                tracing::debug!(link_id = event.link_id, "Click successfully recorded");
            }
            Err(e) => {
                // Нетранзиентная ошибка или исчерпаны все попытки
                metrics::counter!("click_worker_failed_total").increment(1);
                metrics::counter!("click_worker_dropped_total").increment(1);

                tracing::error!(
                    error = ?e,
                    link_id = event.link_id,
                    "Click worker: failed to persist click event after retries"
                );
            }
        }
    }

    tracing::info!("Click worker stopped");
}
