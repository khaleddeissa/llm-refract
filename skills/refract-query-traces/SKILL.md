---
name: refract-query-traces
description: Find recorded Refract executions through the MCP or REST API.
---

# refract-query-traces

Run from the repository root. Use `cargo run -p refract-cli --` in place of `refract` when the CLI is not installed.

Use MCP `search_runs(query, status)` or GET /v1/runs. Search is limited to the latest 100 snapshots, so an absent result does not prove a run never existed. Read known IDs through inspect_run. Native execution snapshots are supported; do not assume OTLP ingestion exists.
