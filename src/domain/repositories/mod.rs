//! Repository trait definitions for the domain layer.
//!
//! This module defines the repository interfaces (traits) that abstract data access
//! operations following the Repository pattern. These traits are implemented by
//! concrete repositories in the infrastructure layer.
//!
//! # Architecture
//!
//! - Traits define the contract for data operations
//! - Implementations live in `crate::infrastructure::persistence`
//! - Mock implementations are auto-generated via `mockall` for testing
//!
//! # Available Repositories
//!
//! - [`LinkRepository`] - Short link CRUD operations
//! - [`ClickStatsReader`] - Click analytics reads (ClickHouse)
//! - [`ClickPublisher`] - Click event publishing (Kafka)
//! - [`DomainRepository`] - Domain management
//! - [`TokenRepository`] - API token authentication
//!
//! # Testing
//!
//! See integration tests in `tests/repository_*.rs` for usage examples.

pub mod click_publisher;
pub mod click_stats_reader;
pub mod domain_repository;
pub mod link_repository;
pub mod stats_repository;
pub mod token_repository;

pub use click_publisher::ClickPublisher;
pub use click_stats_reader::ClickStatsReader;
pub use domain_repository::DomainRepository;
pub use link_repository::LinkRepository;
pub use stats_repository::{DetailedStats, LinkStats, StatsFilter};
pub use token_repository::{ApiToken, TokenRepository};

#[cfg(test)]
pub use click_publisher::MockClickPublisher;
#[cfg(test)]
pub use click_stats_reader::MockClickStatsReader;
#[cfg(test)]
pub use domain_repository::MockDomainRepository;
#[cfg(test)]
pub use link_repository::MockLinkRepository;
#[cfg(test)]
pub use token_repository::MockTokenRepository;
