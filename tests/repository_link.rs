mod common;

use sqlx::PgPool;
use std::sync::Arc;
use url_shortener::domain::entities::NewLink;
use url_shortener::domain::repositories::LinkRepository;
use url_shortener::infrastructure::persistence::PgLinkRepository;

#[sqlx::test]
async fn test_create_link(pool: PgPool) {
    let domain_id = common::create_test_domain(&pool, "test1.com").await;
    let repo = PgLinkRepository::new(Arc::new(pool));

    let new_link = NewLink {
        code: "test123".to_string(),
        long_url: "https://example.com".to_string(),
        domain_id,
        expires_at: None,
        permanent: false,
    };

    let result = repo.create(new_link).await;

    assert!(result.is_ok());
    let link = result.unwrap();
    assert_eq!(link.code, "test123");
    assert_eq!(link.long_url, "https://example.com");
}

#[sqlx::test]
async fn test_find_by_code(pool: PgPool) {
    let domain_id = common::create_test_domain(&pool, "test2.com").await;

    sqlx::query!(
        "INSERT INTO links (code, long_url, domain_id) VALUES ($1, $2, $3)",
        "abc123",
        "https://example.com",
        domain_id
    )
    .execute(&pool)
    .await
    .unwrap();

    let repo = PgLinkRepository::new(Arc::new(pool));
    let result = repo.find_by_code("abc123", domain_id).await;

    assert!(result.is_ok());
    let link = result.unwrap();
    assert!(link.is_some());
    assert_eq!(link.unwrap().code, "abc123");
}

#[sqlx::test]
async fn test_find_by_code_not_found(pool: PgPool) {
    let domain_id = common::create_test_domain(&pool, "test3.com").await;
    let repo = PgLinkRepository::new(Arc::new(pool));

    let result = repo.find_by_code("notfound", domain_id).await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

/// Regression: the same destination URL must be shortenable on two different
/// domains. Previously a global `UNIQUE(long_url)` constraint blocked this.
#[sqlx::test]
async fn test_same_url_allowed_on_different_domains(pool: PgPool) {
    let domain_a = common::create_test_domain(&pool, "doma.com").await;
    let domain_b = common::create_test_domain(&pool, "domb.com").await;
    let repo = PgLinkRepository::new(Arc::new(pool));

    let url = "https://shared-destination.com".to_string();

    let a = repo
        .create(NewLink {
            code: "codea1".to_string(),
            long_url: url.clone(),
            domain_id: domain_a,
            expires_at: None,
            permanent: false,
        })
        .await;
    assert!(a.is_ok(), "first domain create failed: {a:?}");

    let b = repo
        .create(NewLink {
            code: "codeb1".to_string(),
            long_url: url.clone(),
            domain_id: domain_b,
            expires_at: None,
            permanent: false,
        })
        .await;
    assert!(
        b.is_ok(),
        "same URL on a second domain should be allowed: {b:?}"
    );
}

/// Regression: after a link is soft-deleted, the same URL (and code) must be
/// re-creatable. Previously the global/non-partial unique constraints kept the
/// soft-deleted row's URL and blocked re-insertion.
#[sqlx::test]
async fn test_recreate_url_after_soft_delete(pool: PgPool) {
    let domain_id = common::create_test_domain(&pool, "reuse.com").await;
    let repo = PgLinkRepository::new(Arc::new(pool));

    let url = "https://recreate-me.com".to_string();

    repo.create(NewLink {
        code: "reuse1".to_string(),
        long_url: url.clone(),
        domain_id,
        expires_at: None,
        permanent: false,
    })
    .await
    .expect("initial create failed");

    let deleted = repo.soft_delete("reuse1", domain_id).await.unwrap();
    assert!(deleted, "soft_delete should report a row was deleted");

    // Same URL and same code should now be free to reuse.
    let recreated = repo
        .create(NewLink {
            code: "reuse1".to_string(),
            long_url: url.clone(),
            domain_id,
            expires_at: None,
            permanent: false,
        })
        .await;
    assert!(
        recreated.is_ok(),
        "re-creating a soft-deleted URL/code should succeed: {recreated:?}"
    );
}

#[sqlx::test]
async fn test_find_by_long_url(pool: PgPool) {
    let domain_id = common::create_test_domain(&pool, "test4.com").await;

    sqlx::query!(
        "INSERT INTO links (code, long_url, domain_id) VALUES ($1, $2, $3)",
        "xyz789",
        "https://unique-url.com",
        domain_id
    )
    .execute(&pool)
    .await
    .unwrap();

    let repo = PgLinkRepository::new(Arc::new(pool));
    let result = repo
        .find_by_long_url("https://unique-url.com", domain_id)
        .await;

    assert!(result.is_ok());
    let link = result.unwrap();
    assert!(link.is_some());
    assert_eq!(link.unwrap().code, "xyz789");
}
