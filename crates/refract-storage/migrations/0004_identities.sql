CREATE TABLE managed_keys (
    id TEXT PRIMARY KEY NOT NULL,
    organization TEXT NOT NULL,
    project TEXT NOT NULL,
    environment TEXT NOT NULL,
    digest TEXT UNIQUE NOT NULL,
    role TEXT NOT NULL,
    expires_at BIGINT NOT NULL,
    revoked BIGINT NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);
CREATE INDEX managed_key_scope ON managed_keys(organization,project,environment);
CREATE TABLE principals (
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    organization TEXT NOT NULL,
    project TEXT NOT NULL,
    environment TEXT NOT NULL,
    role TEXT NOT NULL,
    enabled BIGINT NOT NULL DEFAULT 1,
    PRIMARY KEY (issuer,subject)
);
