CREATE TABLE project_embeddings (
    organization TEXT NOT NULL,
    project TEXT NOT NULL,
    environment TEXT NOT NULL,
    profile TEXT NOT NULL,
    model TEXT NOT NULL,
    is_default BIGINT NOT NULL,
    auto_index BIGINT NOT NULL,
    PRIMARY KEY (organization,project,environment,profile)
);
CREATE TABLE embedding_jobs (
    organization TEXT NOT NULL,
    project TEXT NOT NULL,
    environment TEXT NOT NULL,
    run_id TEXT NOT NULL,
    profile TEXT NOT NULL,
    model TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    attempts BIGINT NOT NULL DEFAULT 0,
    available_at BIGINT NOT NULL DEFAULT 0,
    lease_token TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (organization,project,environment,run_id,profile)
);
CREATE INDEX embedding_jobs_ready ON embedding_jobs(status,available_at);
