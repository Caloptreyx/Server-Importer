-- Recorded SFTP server imports. Credentials are never stored.
CREATE TABLE dev_caloptreyx_serverimporter_jobs (
    id uuid PRIMARY KEY,
    server_uuid uuid NOT NULL REFERENCES servers(uuid) ON DELETE CASCADE,
    user_uuid uuid REFERENCES users(uuid) ON DELETE SET NULL,
    user_username VARCHAR(255),
    host VARCHAR(255) NOT NULL,
    port INTEGER NOT NULL,
    username VARCHAR(255) NOT NULL,
    auth_method VARCHAR(32) NOT NULL,
    host_fingerprint VARCHAR(128),
    source_path TEXT NOT NULL,
    destination_path TEXT NOT NULL,
    status VARCHAR(32) NOT NULL,
    files_total BIGINT NOT NULL DEFAULT 0,
    files_transferred BIGINT NOT NULL DEFAULT 0,
    files_skipped BIGINT NOT NULL DEFAULT 0,
    files_failed BIGINT NOT NULL DEFAULT 0,
    bytes_total BIGINT NOT NULL DEFAULT 0,
    bytes_transferred BIGINT NOT NULL DEFAULT 0,
    error TEXT,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ
);

CREATE INDEX dev_caloptreyx_serverimporter_jobs_server_idx
    ON dev_caloptreyx_serverimporter_jobs (server_uuid, started_at DESC);

CREATE INDEX dev_caloptreyx_serverimporter_jobs_status_idx
    ON dev_caloptreyx_serverimporter_jobs (status);
