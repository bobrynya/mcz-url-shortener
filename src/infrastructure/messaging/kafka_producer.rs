//! Kafka click publisher (write side) with a no-op fallback.

use std::time::Duration;

use async_trait::async_trait;
use rdkafka::ClientConfig;
use rdkafka::producer::{FutureProducer, FutureRecord, Producer};
use rdkafka::util::Timeout;

use crate::domain::click_event::ClickEvent;
use crate::domain::repositories::ClickPublisher;
use crate::error::AppError;

/// Used when Kafka is not configured: clicks are counted and dropped.
pub struct NoopClickPublisher;

#[async_trait]
impl ClickPublisher for NoopClickPublisher {
    async fn publish(&self, _event: ClickEvent) -> Result<(), AppError> {
        metrics::counter!("click_publish_dropped_total", "reason" => "not_configured").increment(1);
        Ok(())
    }
}

/// Kafka-backed click publisher.
///
/// The `FutureProducer` connects lazily and reconnects internally, so a broker
/// outage doesn't need app-level reconnection logic — failed sends are dropped
/// (logged + metered) and `publish` still returns `Ok(())`.
pub struct KafkaClickPublisher {
    producer: FutureProducer,
    topic: String,
}

impl KafkaClickPublisher {
    /// Builds a producer for the given brokers.
    pub fn new(brokers: &str, topic: String) -> Result<Self, AppError> {
        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", brokers)
            .set("message.timeout.ms", "5000")
            .create()
            .map_err(|e| {
                AppError::internal(
                    "Failed to create Kafka producer",
                    serde_json::json!({ "error": e.to_string() }),
                )
            })?;
        Ok(Self { producer, topic })
    }

    /// Liveness probe for `/health` (non-critical): can we fetch metadata?
    pub fn health_check(&self) -> bool {
        self.producer
            .client()
            .fetch_metadata(Some(&self.topic), Timeout::After(Duration::from_secs(2)))
            .is_ok()
    }
}

#[async_trait]
impl ClickPublisher for KafkaClickPublisher {
    async fn publish(&self, event: ClickEvent) -> Result<(), AppError> {
        let payload = match serde_json::to_vec(&event) {
            Ok(p) => p,
            Err(e) => {
                tracing::error!(error = %e, "Failed to serialize click event");
                metrics::counter!("click_publish_dropped_total", "reason" => "serialize")
                    .increment(1);
                return Ok(());
            }
        };
        let key = event.link_id.to_string();
        let record = FutureRecord::to(&self.topic).payload(&payload).key(&key);

        match self
            .producer
            .send(record, Timeout::After(Duration::from_secs(1)))
            .await
        {
            Ok(_) => {
                metrics::counter!("click_publish_total").increment(1);
            }
            Err((e, _msg)) => {
                tracing::warn!(error = %e, "Kafka publish failed; dropping click");
                metrics::counter!("click_publish_dropped_total", "reason" => "send").increment(1);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[tokio::test]
    async fn test_noop_publisher_ok() {
        let p = NoopClickPublisher;
        let e = ClickEvent::new(1, None, None, None, Utc::now());
        assert!(p.publish(e).await.is_ok());
    }
}
