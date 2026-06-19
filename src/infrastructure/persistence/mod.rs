//! PostgreSQL repository implementations.
//!
//! Concrete implementations of domain repository traits using SQLx for type-safe
//! SQL queries with compile-time verification.
//!
//! # Repositories
//!
//! - [`PgLinkRepository`] - Link storage and retrieval
//! - [`PgDomainRepository`] - Domain management
//! - [`PgTokenRepository`] - API token storage and validation
//! - [`clickhouse_client`] - ClickHouse row type, insert sink, and reconnecting client
//! - [`clickhouse_stats_reader`] - ClickHouse read-side stats implementation

pub mod clickhouse_client;
pub mod clickhouse_stats_reader;
pub mod pg_domain_repository;
pub mod pg_link_repository;
pub mod pg_token_repository;

pub use clickhouse_client::{ClickHouseConfig, ClickRow, ClickSink, ReconnectingClickHouse};
pub use clickhouse_stats_reader::{ClickHouseStatsReader, UnavailableStatsReader};
pub use pg_domain_repository::PgDomainRepository;
pub use pg_link_repository::PgLinkRepository;
pub use pg_token_repository::PgTokenRepository;
