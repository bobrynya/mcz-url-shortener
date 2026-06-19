//! Domain layer containing business entities and logic.
//!
//! This module implements the core domain logic following Clean Architecture principles.
//! It defines entities, repository interfaces, and domain services independent of
//! infrastructure concerns.
//!
//! # Architecture
//!
//! - [`entities`] - Core business data structures
//! - [`repositories`] - Data access trait definitions
//! - [`click_event`] - Click tracking event model
//!
//! # Design Principles
//!
//! - Domain layer has no dependencies on infrastructure or presentation layers
//! - Repository traits define contracts implemented by infrastructure layer
//! - Business logic is encapsulated in services (see [`crate::application::services`])
//!
//! # Click Processing Flow
//!
//! 1. HTTP handler receives redirect request
//! 2. [`click_event::ClickEvent`] is published to Kafka via
//!    [`repositories::ClickPublisher`]
//! 3. A background consumer batches events into ClickHouse
//! 4. Analytics are read back via [`repositories::ClickStatsReader`]

pub mod click_event;
pub mod entities;
pub mod repositories;
