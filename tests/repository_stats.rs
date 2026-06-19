//! PG-level coverage for the link-metadata reads that back the stats endpoints.
//!
//! Click storage moved out of PostgreSQL: the `link_clicks` table and
//! `PgStatsRepository` are gone, and click counts now come from ClickHouse via
//! `ClickStatsReader` (covered by unit tests in
//! `src/application/services/stats_service.rs` and by the handler integration
//! tests in `tests/handler_stats.rs`).
//!
//! What remains testable against Postgres is the link metadata that
//! `StatsService::get_all_stats` / `count_all_links` page over: `LinkRepository::list`
//! and `LinkRepository::count`. The former click-storage tests
//! (`record_click`, `count_clicks_by_link_id`, etc.) were deleted because the
//! behaviour they exercised no longer exists.

mod common;

use std::sync::Arc;

use sqlx::PgPool;
use url_shortener::domain::repositories::LinkRepository;
use url_shortener::infrastructure::persistence::PgLinkRepository;

#[sqlx::test]
async fn test_count_all_links(pool: PgPool) {
    let repo = PgLinkRepository::new(Arc::new(pool.clone()));

    let domain_id = common::create_test_domain(&pool, "count-test.com").await;

    for i in 1..=4 {
        common::create_test_link(
            &pool,
            &format!("count{}", i),
            &format!("https://example.com/{}", i),
            domain_id,
        )
        .await;
    }

    let total = repo.count(None).await.unwrap();
    assert!(total >= 4);

    let scoped = repo.count(Some(domain_id)).await.unwrap();
    assert_eq!(scoped, 4);
}

#[sqlx::test]
async fn test_list_links_for_stats(pool: PgPool) {
    let repo = PgLinkRepository::new(Arc::new(pool.clone()));

    let domain_id = common::create_test_domain(&pool, "list-stats.com").await;
    common::create_test_link(&pool, "meta1", "https://example.com/1", domain_id).await;
    common::create_test_link(&pool, "meta2", "https://example.com/2", domain_id).await;

    let links = repo.list(1, 10, Some(domain_id)).await.unwrap();

    assert_eq!(links.len(), 2);
    let codes: Vec<&str> = links.iter().map(|l| l.code.as_str()).collect();
    assert!(codes.contains(&"meta1"));
    assert!(codes.contains(&"meta2"));

    // Metadata required by the stats merge is populated.
    let meta1 = links.iter().find(|l| l.code == "meta1").unwrap();
    assert_eq!(meta1.long_url, "https://example.com/1");
    assert_eq!(meta1.domain.as_deref(), Some("list-stats.com"));
}
