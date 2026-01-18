//! # mcz-url-shortener
//!
//! Production-ready URL shortener library
//!
//! ## Architecture
//!
//! - **Domain Layer**: Core business entities and logic
//! - **Application Layer**: Use cases and services
//! - **Infrastructure Layer**: Database, caching, external APIs
//! - **API Layer**: HTTP handlers and DTOs

pub mod api;
pub mod application;
pub mod domain;
pub mod error;
pub mod infrastructure;
pub mod state;
pub mod utils;

pub mod config;
pub mod server;

pub mod routes;
pub mod web;

pub use error::AppError;
pub use state::AppState;

pub mod prelude {
    pub use crate::application::services::{AuthService, LinkService, StatsService};
    pub use crate::domain::entities::{Click, Link, NewLink};
    pub use crate::error::AppError;
    pub use crate::state::AppState;
}
