CREATE TABLE trace_assemblies (
    organization TEXT NOT NULL,
    project TEXT NOT NULL,
    environment TEXT NOT NULL,
    trace_id TEXT NOT NULL,
    received_at BIGINT NOT NULL,
    PRIMARY KEY (organization,project,environment,trace_id)
);
CREATE TABLE trace_spans (
    organization TEXT NOT NULL,
    project TEXT NOT NULL,
    environment TEXT NOT NULL,
    trace_id TEXT NOT NULL,
    span_id TEXT NOT NULL,
    payload TEXT NOT NULL,
    PRIMARY KEY (organization,project,environment,trace_id,span_id)
);
CREATE INDEX trace_expiry ON trace_assemblies(received_at);
