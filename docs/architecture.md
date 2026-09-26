# Execution architecture

```mermaid
flowchart TD
  App[Python / Node / OTel] --> Capture[Provider wrappers / spans / explicit events]
  Capture --> Export[Bounded queue / sampling / redaction / retry spool]
  Capture --> Artifact[Readable .rfr]
  Export --> API[Authenticated scoped API / batch ingestion]
  API --> DB[SQLite or PostgreSQL / encrypted payloads / indexed events]
  DB --> Outbox[Durable delivery outbox]
  API --> UI[Graph inspector / search / metrics]
  API --> MCP[MCP evidence tools]
  Artifact --> CLI[CLI and Rust libraries]
  CLI --> Experiment[Explicit executors / model replacement / lineage]
  Experiment --> Artifact
  Artifact --> Eval[Semantic graders / metric budgets / datasets / CI]
```

Runs remain immutable snapshots. Parent events precede their children, and graph visualization uses
recorded relationships. Forking creates a prefix; executable rerun invokes explicitly chosen trusted
application handlers for the suffix. No executable command comes from the artifact itself.

Measurement attributes extend the v1 execution model without a format break. Raw comparison remains
available alongside pluggable output grading and budget evaluation. UTF-8 artifacts preserve the
checksummed payload; Rust still reads legacy ZIP recordings.

See [repository map](repository.md), [production operation](production.md),
[rerun](usage/rerun.md) and [evaluation](usage/evaluation.md) for boundaries and configuration.
