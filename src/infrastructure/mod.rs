//! Infrastructure layer for external integrations.
//!
//! This layer implements interfaces defined by the domain layer, providing
//! concrete implementations for data persistence and caching.
//!
//! # Modules
//!
//! - [`cache`] - Caching abstractions (Redis and no-op implementations)
//! - [`messaging`] - Kafka producer and click consumer
//! - [`persistence`] - PostgreSQL repository implementations

pub mod cache;
pub mod messaging;
pub mod persistence;
