CREATE TABLE IF NOT EXISTS runs (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    status TEXT NOT NULL,
    started_at TEXT NOT NULL,
    execution TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS runs_started_at ON runs(started_at DESC);
