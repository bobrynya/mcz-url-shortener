//! Core domain entities representing the business data model.
//!
//! This module contains the fundamental data structures that represent the core
//! concepts of the URL shortening service. Entities are plain data structures
//! without business logic.
//!
//! # Entity Types
//!
//! - [`Link`] - A shortened URL mapping
//! - [`Click`] - A click event on a shortened link
//! - [`Domain`] - A domain that serves shortened URLs
//!
//! # Design Pattern
//!
//! Entities follow the "New Type" pattern with separate structs for creation:
//! - `NewLink`, `NewDomain` - For creating new records
//! - `UpdateDomain` - For partial updates
//!
//! All entities include unit tests demonstrating their construction and usage.

pub mod click;
pub mod domain;
pub mod link;

pub use click::Click;
pub use domain::{Domain, NewDomain, UpdateDomain};
pub use link::{Link, LinkPatch, NewLink};
