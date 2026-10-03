CREATE TABLE telemetry (
    organization TEXT NOT NULL,
    project TEXT NOT NULL,
    environment TEXT NOT NULL,
    id TEXT NOT NULL,
    kind TEXT NOT NULL,
    trace_id TEXT NOT NULL,
    received_at BIGINT NOT NULL,
    payload TEXT NOT NULL,
    PRIMARY KEY (organization,project,environment,id)
);
CREATE INDEX telemetry_scope_time ON telemetry(organization,project,environment,kind,received_at,id);
CREATE INDEX telemetry_trace ON telemetry(organization,project,environment,trace_id,received_at);
