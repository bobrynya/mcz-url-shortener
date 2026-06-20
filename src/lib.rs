//! # URL Shortener
//!
//! A fast, secure, and scalable URL shortening service built with Axum and PostgreSQL.
//!
//! ## Architecture
//!
//! This crate follows Clean Architecture principles with clear layer separation:
//!
//! - **Domain Layer** ([`domain`]) - Core business entities and repository traits
//! - **Application Layer** ([`application`]) - Business logic and service orchestration
//! - **Infrastructure Layer** ([`infrastructure`]) - Database, cache, and external integrations
//! - **API Layer** ([`api`]) - REST API handlers, DTOs, and middleware
//! - **Web Layer** ([`web`]) - HTML dashboard for link management
//!
//! ## Features
//!
//! - Multi-domain support with custom short codes
//! - Asynchronous click tracking with retry logic
//! - Redis caching for fast redirects
//! - API token authentication
//! - Rate limiting and observability
//!
//! ## Quick Start
//!
//! ```bash
//! # Set required environment variables
//! export DATABASE_URL="postgresql://user:pass@localhost/urlshortener"
//! export REDIS_URL="redis://localhost:6379"  # Optional
//!
//! # Run migrations
//! sqlx migrate run
//!
//! # Start the service
//! cargo run
//! ```
//!
//! ## Testing
//!
//! See [`TESTING.md`](https://github.com/bobrynya/url-shortener/blob/master/TESTING.md)
//! for test coverage and execution instructions.
//!
//! ## Configuration
//!
//! Service configuration is loaded from environment variables via [`config::Config`].
//! See [`config`] module for available options.

pub mod api;
pub mod application;
pub mod domain;
pub mod error;
pub mod infrastructure;
pub mod observability;
pub mod state;
pub mod utils;

pub mod config;
pub mod server;

pub mod routes;
pub mod web;

pub use error::AppError;
pub use state::AppState;

/// Commonly used types for external consumers.
///
/// Re-exports frequently used types to simplify imports for library users
/// and integration tests.
pub mod prelude {
    pub use crate::application::services::{AuthService, LinkService, StatsService};
    pub use crate::domain::entities::{Click, Link, NewLink};
    pub use crate::error::AppError;
    pub use crate::state::AppState;
}
