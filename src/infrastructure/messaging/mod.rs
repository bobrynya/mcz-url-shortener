//! Messaging infrastructure: Kafka producer and click consumer.

pub mod click_consumer;
pub mod kafka_producer;

pub use kafka_producer::{KafkaClickPublisher, NoopClickPublisher};
