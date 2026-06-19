//! Kafka → ClickHouse click consumer with size/time batching.

use std::sync::Arc;
use std::time::Duration;

use rdkafka::Message;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{CommitMode, Consumer, StreamConsumer};
use tokio_util::sync::CancellationToken;

use crate::domain::click_event::ClickEvent;
use crate::infrastructure::persistence::{ClickRow, ClickSink};

/// Decodes a Kafka payload into a click event.
pub fn decode_event(payload: &[u8]) -> Result<ClickEvent, serde_json::Error> {
    serde_json::from_slice(payload)
}

/// Accumulates click rows until the configured capacity is reached.
pub struct BatchBuffer {
    capacity: usize,
    rows: Vec<ClickRow>,
}

impl BatchBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            rows: Vec::with_capacity(capacity.max(1)),
        }
    }

    /// Pushes a row; returns `true` when the buffer is full and should flush.
    pub fn push(&mut self, row: ClickRow) -> bool {
        self.rows.push(row);
        self.rows.len() >= self.capacity
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Drains the buffer, returning the accumulated rows.
    pub fn take(&mut self) -> Vec<ClickRow> {
        std::mem::take(&mut self.rows)
    }
}

/// Runs the Kafka→ClickHouse consumer until `shutdown` is cancelled.
///
/// Batches up to `batch_size` events or every `flush` interval, whichever comes
/// first, then bulk-inserts via `sink`. Offsets are committed only after a
/// successful insert (at-least-once). Malformed messages are skipped + metered.
pub async fn run_click_consumer(
    brokers: String,
    group: String,
    topic: String,
    sink: Arc<dyn ClickSink>,
    batch_size: usize,
    flush: Duration,
    shutdown: CancellationToken,
) {
    let consumer: StreamConsumer = match ClientConfig::new()
        .set("bootstrap.servers", &brokers)
        .set("group.id", &group)
        .set("enable.auto.commit", "false")
        .set("auto.offset.reset", "earliest")
        .create()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "Failed to create Kafka consumer; clicks will not be ingested");
            return;
        }
    };

    if let Err(e) = consumer.subscribe(&[&topic]) {
        tracing::error!(error = %e, topic = %topic, "Failed to subscribe to clicks topic");
        return;
    }
    tracing::info!(topic = %topic, batch_size, "Click consumer started");

    let mut buffer = BatchBuffer::new(batch_size);
    let mut ticker = tokio::time::interval(flush);

    loop {
        tokio::select! {
            _ = shutdown.cancelled() => {
                flush_batch(&consumer, &sink, &mut buffer).await;
                tracing::info!("Click consumer stopped");
                return;
            }
            _ = ticker.tick() => {
                flush_batch(&consumer, &sink, &mut buffer).await;
            }
            msg = consumer.recv() => {
                match msg {
                    Ok(m) => {
                        metrics::counter!("click_consumer_received_total").increment(1);
                        if let Some(payload) = m.payload() {
                            match decode_event(payload) {
                                Ok(event) => {
                                    if buffer.push(ClickRow::from(&event)) {
                                        flush_batch(&consumer, &sink, &mut buffer).await;
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!(error = %e, "Skipping malformed click message");
                                    metrics::counter!("click_consumer_invalid_total").increment(1);
                                    // Skipped messages are still acked via the batch commit below
                                    // (we commit consumer position on the next successful flush).
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Kafka consumer recv error");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        }
    }
}

/// Inserts the buffered rows and, on success, commits consumer offsets.
async fn flush_batch(
    consumer: &StreamConsumer,
    sink: &Arc<dyn ClickSink>,
    buffer: &mut BatchBuffer,
) {
    if buffer.is_empty() {
        return;
    }
    let rows = buffer.take();
    let n = rows.len();
    match sink.insert_batch(&rows).await {
        Ok(()) => {
            metrics::counter!("click_consumer_inserted_total").increment(n as u64);
            metrics::histogram!("click_consumer_batch_size").record(n as f64);
            if let Err(e) = consumer.commit_consumer_state(CommitMode::Async) {
                tracing::warn!(error = %e, "Failed to commit Kafka offsets after insert");
            }
        }
        Err(e) => {
            // Do NOT commit: messages will be re-delivered after ClickHouse recovers.
            tracing::warn!(error = %e, count = n, "ClickHouse insert failed; offsets left uncommitted");
            metrics::counter!("click_consumer_insert_failed_total").increment(1);
            // Re-buffer so we retry the same rows on the next flush.
            for row in rows {
                buffer.push(row);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    fn row(id: u64) -> ClickRow {
        ClickRow {
            link_id: id,
            ip: None,
            user_agent: None,
            referer: None,
            clicked_at: Utc::now(),
        }
    }

    #[test]
    fn test_buffer_signals_full_at_capacity() {
        let mut b = BatchBuffer::new(2);
        assert!(!b.push(row(1)));
        assert!(b.push(row(2)));
        assert_eq!(b.len(), 2);
    }

    #[test]
    fn test_buffer_take_drains() {
        let mut b = BatchBuffer::new(4);
        b.push(row(1));
        b.push(row(2));
        let drained = b.take();
        assert_eq!(drained.len(), 2);
        assert!(b.is_empty());
    }

    #[test]
    fn test_decode_event_round_trip() {
        let e = ClickEvent::new(5, Some("1.1.1.1".into()), None, None, Utc::now());
        let bytes = serde_json::to_vec(&e).unwrap();
        let back = decode_event(&bytes).unwrap();
        assert_eq!(back.link_id, 5);
    }

    #[test]
    fn test_decode_event_rejects_garbage() {
        assert!(decode_event(b"not json").is_err());
    }
}
