-- Shared limits survive restarts and coordinate every process using this database.
CREATE TABLE rate_buckets (
    organization TEXT NOT NULL,
    project TEXT NOT NULL,
    environment TEXT NOT NULL,
    actor TEXT NOT NULL,
    window_start BIGINT NOT NULL,
    requests BIGINT NOT NULL,
    PRIMARY KEY (organization, project, environment, actor)
);
CREATE INDEX rate_bucket_expiry ON rate_buckets(window_start);

-- Embedding payloads use the same authenticated encryption as execution payloads.
CREATE TABLE run_embeddings (
    organization TEXT NOT NULL,
    project TEXT NOT NULL,
    environment TEXT NOT NULL,
    run_id TEXT NOT NULL,
    model TEXT NOT NULL,
    dimensions BIGINT NOT NULL,
    embedding TEXT NOT NULL,
    PRIMARY KEY (organization, project, environment, run_id, model),
    FOREIGN KEY (organization, project, environment, run_id)
        REFERENCES runs(organization, project, environment, id) ON DELETE CASCADE
);
CREATE INDEX embedding_model ON run_embeddings(organization, project, environment, model, dimensions);
ALTER TABLE outbox ADD COLUMN lease_token TEXT NOT NULL DEFAULT '';
