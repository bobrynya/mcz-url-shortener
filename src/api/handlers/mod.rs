//! HTTP request handlers for API endpoints.
//!
//! Each handler module corresponds to a logical grouping of endpoints.

pub mod domains;
pub mod health;
pub mod links;
pub mod redirect;
pub mod stats;

pub use domains::{
    create_domain_handler, delete_domain_handler, domain_list_handler, update_domain_handler,
};
pub use health::health_handler;
pub use links::{
    batch_deactivate_handler, batch_restore_handler, delete_link_handler, shorten_handler,
    update_link_handler,
};
pub use redirect::redirect_handler;
pub use stats::{stats_handler, stats_list_handler};
