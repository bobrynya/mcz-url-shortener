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

#[sqlx::test]
async fn test_soft_delete_many_transitions_only_active(pool: PgPool) {
    let repo = PgLinkRepository::new(Arc::new(pool.clone()));
    let domain_id = common::get_default_domain(&pool).await;

    common::create_test_link(&pool, "bd001", "https://a.com", domain_id).await;
    common::create_test_link(&pool, "bd002", "https://b.com", domain_id).await;
    common::create_deleted_link(&pool, "bd003", "https://c.com", domain_id).await;

    let codes = vec![
        "bd001".to_string(),
        "bd002".to_string(),
        "bd003".to_string(), // already deleted → not transitioned
        "bd404".to_string(), // missing → not transitioned
    ];
    let mut affected = repo.soft_delete_many(&codes, domain_id).await.unwrap();
    affected.sort();

    assert_eq!(affected, vec!["bd001".to_string(), "bd002".to_string()]);

    // Re-running is idempotent: nothing left to transition.
    let again = repo.soft_delete_many(&codes, domain_id).await.unwrap();
    assert!(again.is_empty());
}

#[sqlx::test]
async fn test_soft_delete_many_scoped_to_domain(pool: PgPool) {
    let repo = PgLinkRepository::new(Arc::new(pool.clone()));
    let default_id = common::get_default_domain(&pool).await;
    let other_id = common::create_test_domain(&pool, "other.example.com").await;

    common::create_test_link(&pool, "shared", "https://a.com", default_id).await;
    common::create_test_link(&pool, "shared", "https://b.com", other_id).await;

    let affected = repo
        .soft_delete_many(&["shared".to_string()], default_id)
        .await
        .unwrap();
    assert_eq!(affected, vec!["shared".to_string()]);

    // The other domain's "shared" link is untouched and still deactivatable.
    let other = repo
        .soft_delete_many(&["shared".to_string()], other_id)
        .await
        .unwrap();
    assert_eq!(other, vec!["shared".to_string()]);
}

#[sqlx::test]
async fn test_soft_delete_many_empty_input(pool: PgPool) {
    let repo = PgLinkRepository::new(Arc::new(pool.clone()));
    let domain_id = common::get_default_domain(&pool).await;
    let affected = repo.soft_delete_many(&[], domain_id).await.unwrap();
    assert!(affected.is_empty());
}

#[sqlx::test]
async fn test_restore_many_transitions_only_deleted(pool: PgPool) {
    let repo = PgLinkRepository::new(Arc::new(pool.clone()));
    let domain_id = common::get_default_domain(&pool).await;

    common::create_deleted_link(&pool, "rs001", "https://a.com", domain_id).await;
    common::create_deleted_link(&pool, "rs002", "https://b.com", domain_id).await;
    common::create_test_link(&pool, "rs003", "https://c.com", domain_id).await; // active → not transitioned

    let codes = vec![
        "rs001".to_string(),
        "rs002".to_string(),
        "rs003".to_string(),
        "rs404".to_string(),
    ];
    let mut affected = repo.restore_many(&codes, domain_id).await.unwrap();
    affected.sort();
    assert_eq!(affected, vec!["rs001".to_string(), "rs002".to_string()]);

    // Idempotent: already restored → nothing to transition.
    let again = repo.restore_many(&codes, domain_id).await.unwrap();
    assert!(again.is_empty());
}
