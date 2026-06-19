#![allow(dead_code)]

use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::mpsc;
use url_shortener::application::services::{AuthService, DomainService, LinkService, StatsService};
use url_shortener::infrastructure::cache::NullCache;
use url_shortener::infrastructure::persistence::{
    PgDomainRepository, PgLinkRepository, PgStatsRepository, PgTokenRepository,
};
use url_shortener::state::AppState;

pub async fn create_test_domain(pool: &PgPool, name: &str) -> i64 {
    sqlx::query_scalar!(
        "INSERT INTO domains (domain, is_default) VALUES ($1, false) RETURNING id",
        name
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

pub async fn get_default_domain(pool: &PgPool) -> i64 {
    sqlx::query_scalar!("SELECT id FROM domains WHERE is_default = true LIMIT 1")
        .fetch_one(pool)
        .await
        .unwrap()
}

pub async fn create_test_link(pool: &PgPool, code: &str, url: &str, domain_id: i64) {
    sqlx::query!(
        "INSERT INTO links (code, long_url, domain_id) VALUES ($1, $2, $3)",
        code,
        url,
        domain_id
    )
    .execute(pool)
    .await
    .unwrap();
}

pub async fn create_deleted_link(pool: &PgPool, code: &str, url: &str, domain_id: i64) {
    sqlx::query!(
        "INSERT INTO links (code, long_url, domain_id, deleted_at) VALUES ($1, $2, $3, NOW())",
        code,
        url,
        domain_id
    )
    .execute(pool)
    .await
    .unwrap();
}

pub async fn create_expired_link(pool: &PgPool, code: &str, url: &str, domain_id: i64) {
    sqlx::query!(
        "INSERT INTO links (code, long_url, domain_id, expires_at) VALUES ($1, $2, $3, NOW() - INTERVAL '1 hour')",
        code,
        url,
        domain_id
    )
    .execute(pool)
    .await
    .unwrap();
}

pub async fn create_permanent_link(pool: &PgPool, code: &str, url: &str, domain_id: i64) {
    sqlx::query!(
        "INSERT INTO links (code, long_url, domain_id, permanent) VALUES ($1, $2, $3, TRUE)",
        code,
        url,
        domain_id
    )
    .execute(pool)
    .await
    .unwrap();
}

pub async fn create_test_click(pool: &PgPool, link_id: i64, ip: &str) {
    sqlx::query!(
        "INSERT INTO link_clicks (link_id, ip) VALUES ($1, $2)",
        link_id,
        ip
    )
    .execute(pool)
    .await
    .unwrap();
}

pub fn create_test_state(
    pool: PgPool,
) -> (
    AppState,
    mpsc::Receiver<url_shortener::domain::click_event::ClickEvent>,
) {
    let pool = Arc::new(pool);
    let (tx, rx) = mpsc::channel(100);

    let link_repo = Arc::new(PgLinkRepository::new(pool.clone()));
    let domain_repo = Arc::new(PgDomainRepository::new(pool.clone()));
    let stats_repo = Arc::new(PgStatsRepository::new(pool.clone()));
    let token_repo = Arc::new(PgTokenRepository::new(pool.clone()));

    // Integration tests exercise public URLs; mirror the production default.
    let link_service = Arc::new(LinkService::new(link_repo, domain_repo.clone(), true));
    let domain_service = Arc::new(DomainService::new(domain_repo));
    let stats_service = Arc::new(StatsService::new(stats_repo));
    let auth_service = Arc::new(AuthService::new(
        token_repo,
        "test-signing-secret".to_string(),
    ));

    let state = AppState {
        link_service,
        stats_service,
        auth_service,
        domain_service,
        cache: Arc::new(NullCache),
        click_sender: tx,
        cookie_secure: false,
    };

    (state, rx)
}
