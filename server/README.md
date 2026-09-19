# Rust server

Implementation: [crates/refract-server/src/lib.rs](../crates/refract-server/src/lib.rs).
Storage: [crates/refract-storage](../crates/refract-storage). Embedded SQLx migrations: [migrations](../migrations).

```bash
sh server/run.sh
# Or use the packaged server + UI:
docker compose up --build -d --wait
```

The server implements native ingestion, validation/redaction, SQLite persistence, run/event reads,
recorded replay, fork creation, structural diff, artifact downloads, health/readiness and static UI serving.
See [API reference](../docs/api.md) for routes, request bodies and configuration.
