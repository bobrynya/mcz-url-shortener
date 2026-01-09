use sqlx::{Error as SqlxError, PgPool};
use tokio::sync::mpsc;
use tokio_retry::strategy::ExponentialBackoff;
use tokio_retry::RetryIf;

use crate::domain::click_event::ClickEvent;

fn is_transient_sqlx_error(e: &SqlxError) -> bool {
    // Базовая эвристика:
    // - проблемы с сетью/соединением/пулом чаще всего транзиентны
    // - “ошибка запроса/ограничений” — нет
    matches!(
        e,
        SqlxError::Io(_) | SqlxError::PoolTimedOut | SqlxError::PoolClosed
    )
}

pub async fn run_click_worker(mut rx: mpsc::Receiver<ClickEvent>, db: PgPool) {
    while let Some(ev) = rx.recv().await {
        metrics::counter!("click_worker_received_total").increment(1);

        // стратегия: 100ms, 200ms, 400ms, ... с ограничением числа попыток
        let strategy = ExponentialBackoff::from_millis(100).take(6);

        let op = || {
            let db = db.clone();
            let ev = ev.clone();
            async move {
                sqlx::query!(
                    r#"
                    INSERT INTO link_clicks (link_id, clicked_at, referer, user_agent, ip)
                    VALUES ($1, $2, $3, $4, $5)
                    "#,
                    ev.link_id,
                    ev.clicked_at,
                    ev.referer,
                    ev.user_agent,
                    ev.ip,
                )
                .execute(&db)
                .await
                .map(|_| ())
            }
        };

        let on_error = |e: &SqlxError| {
            let transient = is_transient_sqlx_error(e);
            if transient {
                metrics::counter!("click_worker_retried_total").increment(1);
            }
            transient
        };

        let res = RetryIf::spawn(strategy, op, on_error).await;

        match res {
            Ok(()) => {
                metrics::counter!("click_worker_processed_total").increment(1);
            }
            Err(e) => {
                // сюда попадают либо нетранзиентные, либо “транзиентные, но попытки закончились”
                metrics::counter!("click_worker_failed_total").increment(1);

                // Сейчас — считаем как dropped.
                metrics::counter!("click_worker_dropped_total").increment(1);

                tracing::error!(error = %e, link_id = ev.link_id, "click_worker: failed to persist click event");
            }
        }
    }
}
