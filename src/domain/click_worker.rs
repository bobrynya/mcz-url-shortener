//! Background worker for processing click events asynchronously.

use serde_json::json;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_retry::RetryIf;
use tokio_retry::strategy::ExponentialBackoff;

use crate::domain::click_event::ClickEvent;
use crate::domain::entities::NewClick;
use crate::domain::repositories::{DomainRepository, LinkRepository, StatsRepository};
use crate::error::AppError;

/// Returns `true` for transient errors that are worth retrying (e.g. DB connection issues).
///
/// Permanent errors such as "link not found" return `false` and are not retried.
fn is_transient_error(e: &AppError) -> bool {
    matches!(e, AppError::Internal { .. })
}

/// Persists a single click event, resolving domain → link → click record.
///
/// Retries up to 6 times with exponential backoff (100 ms → 3.2 s) on transient errors.
/// Permanent errors (domain/link not found) are logged and discarded immediately.
///
/// # Metrics
///
/// - `click_worker_processed_total` - incremented on success
/// - `click_worker_retried_total`   - incremented on each retry attempt
/// - `click_worker_failed_total`    - incremented after exhausting all retries
/// - `click_worker_dropped_total`   - incremented when the event is discarded
async fn process_click<S, D, L>(
    event: ClickEvent,
    stats_repository: Arc<S>,
    domain_repository: Arc<D>,
    link_repository: Arc<L>,
) where
    S: StatsRepository,
    D: DomainRepository,
    L: LinkRepository,
{
    let strategy = ExponentialBackoff::from_millis(100).take(6);

    let stats_repo = stats_repository.clone();
    let domain_repo = domain_repository.clone();
    let link_repo = link_repository.clone();
    let ev = event.clone();

    let op = || {
        let stats_repo = stats_repo.clone();
        let domain_repo = domain_repo.clone();
        let link_repo = link_repo.clone();
        let event = ev.clone();

        async move {
            let domain_entity =
                domain_repo
                    .find_by_name(&event.domain)
                    .await?
                    .ok_or_else(|| {
                        AppError::not_found(
                            format!("Domain not found: {}", event.domain),
                            json!({ "domain": event.domain.clone() }),
                        )
                    })?;

            let link = link_repo
                .find_by_code(&event.code, domain_entity.id)
                .await?
                .ok_or_else(|| {
                    AppError::not_found(
                        format!("Link not found: {}", event.code),
                        json!({ "code": event.code.clone(), "domain_id": domain_entity.id }),
                    )
                })?;

            let new_click = NewClick {
                link_id: link.id,
                user_agent: event.user_agent,
                referer: event.referer,
                ip: event.ip,
            };

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

    match RetryIf::start(strategy, op, on_error).await {
        Ok(()) => {
            metrics::counter!("click_worker_processed_total").increment(1);
            tracing::debug!(
                domain = &event.domain,
                code = &event.code,
                "Click successfully recorded"
            );
        }
        Err(e) => {
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

/// Runs the background click processing worker with bounded concurrency.
///
/// Reads [`ClickEvent`]s from `rx` and processes up to `concurrency` events in parallel.
/// Each event is handled by [`process_click`], which retries transient database errors
/// with exponential backoff.
///
/// # Concurrency
///
/// At most `concurrency` events are in-flight simultaneously. When all slots are busy,
/// the worker waits for one to finish before accepting the next event. The mpsc channel
/// buffer (configured via `CLICK_QUEUE_CAPACITY`) absorbs bursts beyond this limit.
///
/// # Graceful Shutdown
///
/// The worker exits when the sending side of the channel is dropped (i.e. after
/// `axum::serve` completes and [`crate::state::AppState`] is deallocated).
/// Before returning, all in-flight tasks are drained to avoid losing events.
///
/// # Metrics
///
/// - `click_worker_received_total` - events received from channel
/// - `click_worker_processed_total` - events successfully persisted
/// - `click_worker_retried_total` - individual retry attempts
/// - `click_worker_failed_total` - events that exhausted all retries
/// - `click_worker_dropped_total` - events discarded due to permanent errors
pub async fn run_click_worker<S, D, L>(
    mut rx: mpsc::Receiver<ClickEvent>,
    stats_repository: Arc<S>,
    domain_repository: Arc<D>,
    link_repository: Arc<L>,
    concurrency: usize,
) where
    S: StatsRepository + 'static,
    D: DomainRepository + 'static,
    L: LinkRepository + 'static,
{
    tracing::info!(concurrency, "Click worker started");

    let mut join_set: JoinSet<()> = JoinSet::new();

    while let Some(ev) = rx.recv().await {
        metrics::counter!("click_worker_received_total").increment(1);

        // Clean up already-finished tasks to keep join_set size accurate.
        while join_set.try_join_next().is_some() {}

        // If at capacity, wait for one slot to free up before spawning more.
        if join_set.len() >= concurrency {
            join_set.join_next().await;
        }

        let stats_repo = stats_repository.clone();
        let domain_repo = domain_repository.clone();
        let link_repo = link_repository.clone();

        join_set.spawn(async move {
            process_click(ev, stats_repo, domain_repo, link_repo).await;
        });
    }

    // Drain all in-flight tasks before returning so no events are lost on shutdown.
    while join_set.join_next().await.is_some() {}

    tracing::info!("Click worker stopped");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::{Click, Domain, Link};
    use crate::domain::repositories::{
        MockDomainRepository, MockLinkRepository, MockStatsRepository,
    };
    use chrono::Utc;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn test_click_worker_successful_processing() {
        let mut mock_domain_repo = MockDomainRepository::new();
        let mut mock_link_repo = MockLinkRepository::new();
        let mut mock_stats_repo = MockStatsRepository::new();

        let domain = Domain::new(
            1,
            "s.example.com".to_string(),
            true,
            true,
            None,
            Utc::now(),
            Utc::now(),
            None,
        );
        mock_domain_repo
            .expect_find_by_name()
            .withf(|name| name == "s.example.com")
            .times(1)
            .returning(move |_| Ok(Some(domain.clone())));

        let link = Link::new(
            10,
            "abc123".to_string(),
            "https://example.com".to_string(),
            Some("s.example.com".to_string()),
            Utc::now(),
            None,
            false,
            None,
        );
        mock_link_repo
            .expect_find_by_code()
            .withf(|code, domain_id| code == "abc123" && *domain_id == 1)
            .times(1)
            .returning(move |_, _| Ok(Some(link.clone())));

        let click = Click::new(1, 10, Utc::now(), None, None, None);
        mock_stats_repo
            .expect_record_click()
            .times(1)
            .returning(move |_| Ok(click.clone()));

        let (tx, rx) = mpsc::channel(10);

        let stats_repo = Arc::new(mock_stats_repo);
        let domain_repo = Arc::new(mock_domain_repo);
        let link_repo = Arc::new(mock_link_repo);

        let worker_handle =
            tokio::spawn(run_click_worker(rx, stats_repo, domain_repo, link_repo, 4));

        let event = ClickEvent::new(
            "s.example.com".to_string(),
            "abc123".to_string(),
            Some("192.168.1.1".to_string()),
            Some("Mozilla/5.0"),
            None,
        );
        tx.send(event).await.unwrap();

        drop(tx);
        worker_handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_click_worker_domain_not_found() {
        let mut mock_domain_repo = MockDomainRepository::new();
        let mock_link_repo = MockLinkRepository::new();
        let mock_stats_repo = MockStatsRepository::new();

        mock_domain_repo
            .expect_find_by_name()
            .times(1)
            .returning(|_| Ok(None));

        let (tx, rx) = mpsc::channel(10);

        let stats_repo = Arc::new(mock_stats_repo);
        let domain_repo = Arc::new(mock_domain_repo);
        let link_repo = Arc::new(mock_link_repo);

        let worker_handle =
            tokio::spawn(run_click_worker(rx, stats_repo, domain_repo, link_repo, 4));

        let event = ClickEvent::new(
            "nonexistent.com".to_string(),
            "abc123".to_string(),
            None,
            None,
            None,
        );
        tx.send(event).await.unwrap();

        drop(tx);
        worker_handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_click_worker_link_not_found() {
        let mut mock_domain_repo = MockDomainRepository::new();
        let mut mock_link_repo = MockLinkRepository::new();
        let mock_stats_repo = MockStatsRepository::new();

        let domain = Domain::new(
            1,
            "s.example.com".to_string(),
            true,
            true,
            None,
            Utc::now(),
            Utc::now(),
            None,
        );
        mock_domain_repo
            .expect_find_by_name()
            .times(1)
            .returning(move |_| Ok(Some(domain.clone())));

        mock_link_repo
            .expect_find_by_code()
            .times(1)
            .returning(|_, _| Ok(None));

        let (tx, rx) = mpsc::channel(10);

        let stats_repo = Arc::new(mock_stats_repo);
        let domain_repo = Arc::new(mock_domain_repo);
        let link_repo = Arc::new(mock_link_repo);

        let worker_handle =
            tokio::spawn(run_click_worker(rx, stats_repo, domain_repo, link_repo, 4));

        let event = ClickEvent::new(
            "s.example.com".to_string(),
            "nonexistent".to_string(),
            None,
            None,
            None,
        );
        tx.send(event).await.unwrap();

        drop(tx);
        worker_handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_click_worker_processes_multiple_events() {
        let mut mock_domain_repo = MockDomainRepository::new();
        let mut mock_link_repo = MockLinkRepository::new();
        let mut mock_stats_repo = MockStatsRepository::new();

        let domain = Domain::new(
            1,
            "s.example.com".to_string(),
            true,
            true,
            None,
            Utc::now(),
            Utc::now(),
            None,
        );
        mock_domain_repo
            .expect_find_by_name()
            .times(3)
            .returning(move |_| Ok(Some(domain.clone())));

        let link = Link::new(
            10,
            "abc123".to_string(),
            "https://example.com".to_string(),
            Some("s.example.com".to_string()),
            Utc::now(),
            None,
            false,
            None,
        );
        mock_link_repo
            .expect_find_by_code()
            .times(3)
            .returning(move |_, _| Ok(Some(link.clone())));

        let click = Click::new(1, 10, Utc::now(), None, None, None);
        mock_stats_repo
            .expect_record_click()
            .times(3)
            .returning(move |_| Ok(click.clone()));

        let (tx, rx) = mpsc::channel(10);

        let stats_repo = Arc::new(mock_stats_repo);
        let domain_repo = Arc::new(mock_domain_repo);
        let link_repo = Arc::new(mock_link_repo);

        let worker_handle =
            tokio::spawn(run_click_worker(rx, stats_repo, domain_repo, link_repo, 4));

        for _ in 0..3 {
            let event = ClickEvent::new(
                "s.example.com".to_string(),
                "abc123".to_string(),
                Some("192.168.1.1".to_string()),
                None,
                None,
            );
            tx.send(event).await.unwrap();
        }

        drop(tx);
        worker_handle.await.unwrap();
    }
}
