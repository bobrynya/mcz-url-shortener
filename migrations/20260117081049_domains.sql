-- Таблица доменов
CREATE TABLE domains (
    id BIGSERIAL PRIMARY KEY,
    domain VARCHAR(255) NOT NULL UNIQUE,
    is_default BOOLEAN NOT NULL DEFAULT FALSE,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,

    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_domains_is_default ON domains(is_default) WHERE is_default = TRUE;
CREATE INDEX idx_domains_is_active ON domains(is_active);

CREATE UNIQUE INDEX idx_domains_single_default
    ON domains(is_default)
    WHERE is_default = TRUE;

ALTER TABLE links ADD COLUMN domain_id BIGINT REFERENCES domains(id);

CREATE INDEX idx_links_code_domain ON links(code, domain_id);

ALTER TABLE links DROP CONSTRAINT IF EXISTS links_code_key;

CREATE UNIQUE INDEX idx_links_unique_code_per_domain
    ON links(code, COALESCE(domain_id, 0));

CREATE OR REPLACE FUNCTION update_updated_at_column()
    RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER update_domains_updated_at
    BEFORE UPDATE ON domains
    FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column();

INSERT INTO domains (domain, is_default, is_active, description)
VALUES ('s.example.com', TRUE, TRUE, 'Default domain')
ON CONFLICT (domain) DO NOTHING;

-- Получаем ID default домена
DO $$
    DECLARE
        default_domain_id BIGINT;
    BEGIN
        SELECT id INTO default_domain_id FROM domains WHERE is_default = TRUE LIMIT 1;

        -- Миграция существующих ссылок к default домену
        UPDATE links
        SET domain_id = default_domain_id
        WHERE domain_id IS NULL;
    END $$;
