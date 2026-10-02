-- Persistent per-run sequence, including tombstones, fences delayed external deliveries.
CREATE TABLE delivery_versions (
    organization TEXT NOT NULL,
    project TEXT NOT NULL,
    environment TEXT NOT NULL,
    run_id TEXT NOT NULL,
    version BIGINT NOT NULL,
    PRIMARY KEY (organization, project, environment, run_id)
);
ALTER TABLE outbox ADD COLUMN version BIGINT NOT NULL DEFAULT 0;
