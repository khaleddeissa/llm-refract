-- Authentication registries are accessed before tenant identity is established.
CREATE TABLE browser_logins (
    id TEXT PRIMARY KEY,
    binding TEXT NOT NULL,
    payload TEXT NOT NULL,
    expires_at BIGINT NOT NULL
);
CREATE TABLE browser_sessions (
    id TEXT PRIMARY KEY,
    organization TEXT NOT NULL,
    project TEXT NOT NULL,
    environment TEXT NOT NULL,
    payload TEXT NOT NULL,
    expires_at BIGINT NOT NULL,
    last_seen BIGINT NOT NULL,
    lease TEXT NOT NULL DEFAULT '',
    lease_until BIGINT NOT NULL DEFAULT 0
);
CREATE INDEX browser_session_expiry ON browser_sessions(expires_at, last_seen);
