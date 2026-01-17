pub mod domains;
pub mod health;
pub mod redirect;
pub mod shorten;
pub mod stats;
pub mod stats_list;

pub use domains::domain_list_handler;
pub use health::health_handler;
pub use redirect::redirect_handler;
pub use shorten::shorten_handler;
pub use stats::stats_handler;
pub use stats_list::stats_list_handler;
