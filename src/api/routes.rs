//! API route configuration.
//!
//! All API endpoints require Bearer token authentication via
//! [`crate::api::middleware::auth`].

use crate::api::handlers::{
    batch_deactivate_handler, batch_restore_handler, create_domain_handler, delete_domain_handler,
    delete_link_handler, domain_list_handler, shorten_handler, stats_handler, stats_list_handler,
    update_domain_handler, update_link_handler,
};
use crate::state::AppState;
use axum::{
    Router,
    routing::{delete, get, patch, post},
};

/// All API routes, protected by Bearer token authentication.
///
/// # Endpoints
///
/// - `GET    /domains`        - List configured domains
/// - `POST   /domains`        - Create a new domain
/// - `PATCH  /domains/{id}`   - Update a domain (rename, toggle active/default, etc.)
/// - `DELETE /domains/{id}`   - Soft-delete a domain
/// - `GET    /stats`          - Aggregated click statistics (paginated)
/// - `GET    /stats/{code}`   - Detailed statistics for a specific link
/// - `POST   /shorten`        - Create shortened URLs (batch-capable)
/// - `DELETE /links/{code}`        - Soft-delete a link
/// - `PATCH  /links/{code}`        - Partially update a link
/// - `POST   /links/batch-deactivate` - Bulk soft-delete links
/// - `POST   /links/batch-restore`    - Bulk restore soft-deleted links
pub fn protected_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/domains",
            get(domain_list_handler).post(create_domain_handler),
        )
        .route(
            "/domains/{id}",
            patch(update_domain_handler).delete(delete_domain_handler),
        )
        .route("/stats", get(stats_list_handler))
        .route("/stats/{code}", get(stats_handler))
        .route("/shorten", post(shorten_handler))
        .route(
            "/links/{code}",
            delete(delete_link_handler).patch(update_link_handler),
        )
        .route("/links/batch-deactivate", post(batch_deactivate_handler))
        .route("/links/batch-restore", post(batch_restore_handler))
}
