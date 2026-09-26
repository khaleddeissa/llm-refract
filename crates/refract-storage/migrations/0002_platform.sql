ALTER TABLE runs RENAME TO legacy_runs;
DROP INDEX IF EXISTS runs_started_at;

CREATE TABLE runs (
    organization TEXT NOT NULL,
    project TEXT NOT NULL,
    environment TEXT NOT NULL,
    id TEXT NOT NULL,
    name TEXT NOT NULL,
    status TEXT NOT NULL,
    started_at TEXT NOT NULL,
    received_at TEXT NOT NULL,
    duration_ms DOUBLE PRECISION NOT NULL DEFAULT 0,
    cost_usd DOUBLE PRECISION NOT NULL DEFAULT 0,
    execution TEXT NOT NULL,
    indexed BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (organization, project, environment, id)
);
INSERT INTO runs(organization,project,environment,id,name,status,started_at,received_at,execution)
    SELECT 'local','default','development',id,name,REPLACE(status,'"',''),started_at,started_at,execution
    FROM legacy_runs;
DROP TABLE legacy_runs;
CREATE INDEX runs_scope_started ON runs(organization, project, environment, started_at DESC, id DESC);
CREATE INDEX runs_scope_status ON runs(organization, project, environment, status, started_at DESC);

CREATE TABLE run_events (
    organization TEXT NOT NULL,
    project TEXT NOT NULL,
    environment TEXT NOT NULL,
    run_id TEXT NOT NULL,
    event_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    model TEXT NOT NULL,
    duration_ms DOUBLE PRECISION NOT NULL,
    PRIMARY KEY (organization, project, environment, run_id, event_id),
    FOREIGN KEY (organization, project, environment, run_id)
        REFERENCES runs(organization, project, environment, id) ON DELETE CASCADE
);
CREATE INDEX events_model ON run_events(organization, project, environment, model, run_id);
CREATE INDEX events_tool ON run_events(organization, project, environment, kind, name, run_id);

CREATE TABLE audit_log (
    id TEXT PRIMARY KEY NOT NULL,
    organization TEXT NOT NULL,
    project TEXT NOT NULL,
    environment TEXT NOT NULL,
    actor TEXT NOT NULL,
    action TEXT NOT NULL,
    resource TEXT NOT NULL,
    status BIGINT NOT NULL,
    timestamp TEXT NOT NULL
);
CREATE INDEX audit_scope ON audit_log(organization, project, environment, timestamp DESC);

CREATE TABLE outbox (
    id TEXT PRIMARY KEY NOT NULL,
    organization TEXT NOT NULL,
    project TEXT NOT NULL,
    environment TEXT NOT NULL,
    run_id TEXT NOT NULL,
    target TEXT NOT NULL,
    operation TEXT NOT NULL,
    attempts BIGINT NOT NULL DEFAULT 0,
    available_at BIGINT NOT NULL DEFAULT 0,
    UNIQUE (organization, project, environment, run_id, target, operation)
);
CREATE INDEX outbox_ready ON outbox(available_at, id);
