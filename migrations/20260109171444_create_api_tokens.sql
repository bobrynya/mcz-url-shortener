-- Долгоживущие токены (храним только хэш)
CREATE TABLE IF NOT EXISTS api_tokens (
    id           BIGSERIAL PRIMARY KEY,
    name         TEXT NOT NULL,
    token_hash   TEXT NOT NULL UNIQUE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ NULL,
    revoked_at   TIMESTAMPTZ NULL
);

CREATE INDEX IF NOT EXISTS api_tokens_token_hash_idx
    ON api_tokens (token_hash);

CREATE INDEX IF NOT EXISTS api_tokens_revoked_at_idx
    ON api_tokens (revoked_at);
