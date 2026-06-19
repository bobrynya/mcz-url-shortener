CREATE DATABASE IF NOT EXISTS url_shortener;

CREATE TABLE IF NOT EXISTS url_shortener.clicks (
    link_id    UInt64,
    ip         Nullable(String),
    user_agent Nullable(String),
    referer    Nullable(String),
    clicked_at DateTime64(3, 'UTC')
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(clicked_at)
ORDER BY (link_id, clicked_at);
