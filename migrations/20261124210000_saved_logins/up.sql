-- User-owned saved SFTP logins. Secrets are encrypted at rest with the panel's APP_ENCRYPTION_KEY.
CREATE TABLE dev_caloptreyx_serverimporter_logins (
    id uuid PRIMARY KEY,
    user_uuid uuid NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    name VARCHAR(128) NOT NULL,
    host VARCHAR(255) NOT NULL,
    port INTEGER NOT NULL,
    username VARCHAR(255) NOT NULL,
    auth_method VARCHAR(32) NOT NULL,
    secret_encrypted BYTEA NOT NULL,
    passphrase_encrypted BYTEA,
    default_source_path TEXT NOT NULL DEFAULT '/',
    host_fingerprint VARCHAR(128),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ
);

CREATE INDEX dev_caloptreyx_serverimporter_logins_user_idx
    ON dev_caloptreyx_serverimporter_logins (user_uuid, updated_at DESC);
