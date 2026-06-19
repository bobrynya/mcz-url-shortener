//! HTML template rendering handlers for the web dashboard.

mod dashboard;
mod domains;
mod links;
mod login;
mod stats;

pub use dashboard::dashboard_handler;
pub use domains::domains_handler;
pub use links::links_handler;
pub use login::{login_handler, login_submit_handler, logout_handler};
pub use stats::stats_handler;
