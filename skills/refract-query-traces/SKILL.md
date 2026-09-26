---
name: refract-query-traces
description: Find recorded Refract executions through the MCP or REST API.
---

# refract-query-traces

Run from the repository root. Use `cargo run -p refract-cli --` in place of `refract` when the CLI is not installed.

Use MCP `search_runs` or `GET /v1/search` with name/ID, status, model, tool and minimum-duration filters.
Paginate with `limit` and `offset`; results belong to the authenticated API key's scope. An absent result
can mean a different scope or retention, not proof that an execution never existed. Read known IDs with
`inspect_run`, use `similar_runs` for bounded lexical discovery and `run_metrics` to inspect usage.
Configure `REFRACT_API_KEY` in the MCP environment for secured services; never paste keys into tool
arguments or recordings. OTel JSON bridges are SDK functionality, not native protobuf ingestion.
