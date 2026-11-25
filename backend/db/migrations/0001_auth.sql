-- Sign-in tables (decision 0014).

CREATE TABLE IF NOT EXISTS employees (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    auth0_id    text NULL,
    email       text NOT NULL,
    name        text NOT NULL,
    is_active   boolean NOT NULL DEFAULT true,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    deleted_at  timestamptz NULL
);

-- The website created this table first (its own staff list); the shared shape must accept Space rows that
-- are pre-provisioned before sign-in, so auth0_id is nullable. Email is unique across both apps.
ALTER TABLE employees ALTER COLUMN auth0_id DROP NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS employees_email_key ON employees (email);
CREATE UNIQUE INDEX IF NOT EXISTS employees_auth0_id_key ON employees (auth0_id);

CREATE TABLE IF NOT EXISTS sessions (
    id            uuid PRIMARY KEY,
    user_id       uuid NOT NULL REFERENCES employees (id),
    token_hash    bytea NOT NULL UNIQUE,
    created_at    timestamptz NOT NULL DEFAULT now(),
    last_used_at  timestamptz NOT NULL DEFAULT now(),
    expires_at    timestamptz NOT NULL,
    revoked_at    timestamptz NULL
);

CREATE TABLE IF NOT EXISTS login_codes (
    code_hash      bytea PRIMARY KEY,
    user_id        uuid NOT NULL REFERENCES employees (id),
    code_challenge text NOT NULL,
    expires_at     timestamptz NOT NULL,
    used_at        timestamptz NULL
);
