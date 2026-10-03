# REST API

The API accepts canonical JSON executions from any provider or language. A server is optional for local
`.rfr` workflows. Requests/responses are JSON except artifact downloads; application errors return
`{"error":"message"}`. Axum handles malformed JSON/content types using its own HTTP error responses.

With API keys configured, send `Authorization: Bearer YOUR_KEY`. Each key fixes the organization,
project, environment and role; client headers cannot override them. Reader keys can inspect, compare,
evaluate and perform recorded playback. Writer keys also ingest/fork. Admin keys additionally access
administrative routes within the same scope. `/v1/health` and `/v1/ready` are unauthenticated.

| Method | Path                                 | Behavior                                                            |
| ------ | ------------------------------------ | ------------------------------------------------------------------- |
| GET    | `/v1/health`                         | Process health                                                      |
| GET    | `/v1/ready`                          | Database connectivity; 503 when unavailable                         |
| POST   | `/v1/runs`                           | Validate/redact/insert one immutable snapshot; 201 or duplicate 409 |
| POST   | `/v1/runs/batch`                     | Atomic, content-checked idempotent batch of 1..1000 snapshots       |
| GET    | `/v1/runs`                           | Latest 100 snapshots in the key's scope                             |
| GET    | `/v1/search`                         | Filter and paginate indexed runs; see [search](usage/search.md)     |
| GET    | `/v1/runs/{id}`                      | Complete snapshot or 404                                            |
| GET    | `/v1/runs/{id}/events`               | Ordered events                                                      |
| GET    | `/v1/runs/{id}/metrics`              | Recorded cost, tokens, latency and completeness                     |
| GET    | `/v1/runs/{id}/similar?limit=10`     | Lexical Jaccard similarity against latest 1000 scoped candidates    |
| GET    | `/v1/runs/{id}/artifact`             | Download a checksummed readable `.rfr`                              |
| POST   | `/v1/runs/{id}/replay`               | `{"mode":"exact"}`; recorded steps only                             |
| POST   | `/v1/runs/{id}/fork`                 | `{"from_event":"evt_2"}`; persist a prefix snapshot                 |
| POST   | `/v1/diff`                           | Structural differences, metric changes and optional semantic report |
| POST   | `/v1/eval`                           | Compare 1..100 named pairs of stored runs with optional budgets     |
| GET    | `/v1/admin/audit?limit=100&offset=0` | Scoped authenticated request audit entries; admin only              |
| POST   | `/v1/admin/retention`                | `{"days":30}`; delete runs received before the cutoff; admin only   |
| GET    | `/v1/admin/outbox`                   | `{"pending":N}` scoped delivery backlog; admin only                 |

Requests are limited to 16 MiB. Invalid application data returns 400, invalid keys 401, insufficient
roles 403, absent/cross-scope runs 404, snapshot conflicts 409 and rate limits 429 with `Retry-After: 60`.
Unknown body fields are rejected. Missing metric observations remain explicit; no pricing catalogue is
used to invent costs. The similar-runs route uses lexical ranking; `/v1/search/vector` provides scoped cosine retrieval with exact and HNSW modes. `/v1/search/text` embeds a query using a configured project profile.

## HTTP examples

For local Compose use `http://localhost:8000`; production uses your HTTPS hostname and scoped key.
Omit the Authorization header only when running the unauthenticated local service.

```bash
export REFRACT_URL=http://localhost:8000
curl --fail "$REFRACT_URL/v1/ready"
curl --fail -X POST "$REFRACT_URL/v1/runs" \
  -H "Authorization: Bearer $REFRACT_API_KEY" -H 'Content-Type: application/json' \
  --data-binary @tests/fixtures/simple-run/execution.json
curl --fail "$REFRACT_URL/v1/runs/demo-1/metrics" \
  -H "Authorization: Bearer $REFRACT_API_KEY"
curl --fail -X POST "$REFRACT_URL/v1/runs/demo-1/replay" \
  -H "Authorization: Bearer $REFRACT_API_KEY" -H 'Content-Type: application/json' \
  -d '{"mode":"exact"}'
mkdir -p .examples
curl --fail --output .examples/api-export.rfr "$REFRACT_URL/v1/runs/demo-1/artifact" \
  -H "Authorization: Bearer $REFRACT_API_KEY"
```

Batch request shape is `{"runs":[<execution>,<execution>]}`. A success returns
`{"accepted":1,"duplicates":1,"run_ids":["run_a","run_a"]}`. Exact duplicates are accepted;
reusing an ID for different normalized content returns 409 and rolls back the entire batch.
Single-run POST retains strict duplicate rejection, even for identical content.

```bash
curl --fail -X POST "$REFRACT_URL/v1/diff" \
  -H "Authorization: Bearer $REFRACT_API_KEY" -H 'Content-Type: application/json' \
  -d '{"left":"demo-1","right":"demo-1","semantic":true,
       "options":{"similarity_threshold":0.8,"max_cost_increase_percent":10}}'
curl --fail -X POST "$REFRACT_URL/v1/eval" \
  -H "Authorization: Bearer $REFRACT_API_KEY" -H 'Content-Type: application/json' \
  -d '{"pairs":[{"name":"baseline","left":"demo-1","right":"demo-1"}],
       "options":{"max_latency_increase_percent":20,"max_token_increase_percent":10}}'
```

A diff response contains `first_divergence`, `differences`, `metric_changes` and `semantic_report`
(null unless requested). Evaluation returns `passed`, `total`, `regressions`, `equivalent` and named
`results`. These endpoints use the offline grader by default. Set `grader` to a configured profile and
`allow_live:true` to use its domain rubric and model. Dataset manifests are supported by local
[evaluation](usage/evaluation.md).

The export content type is `application/vnd.refract.rfr`. POST ingestion expects execution JSON,
not the artifact header/body encoding: decode/validate the file first. Native HTTP/protobuf and gRPC trace receivers are documented in [OpenTelemetry](usage/otel.md).
[Production configuration](production.md) documents secrets, roles, TLS, databases and delivery.

## Service extension routes

| Method     | Path                          | Purpose                                          |
| ---------- | ----------------------------- | ------------------------------------------------ |
| GET        | `/v1/auth/config`             | Public browser SSO configuration; no credentials |
| POST       | `/v1/traces`                  | OTLP HTTP JSON/protobuf ingestion; writer        |
| PUT        | `/v1/runs/{id}/embedding`     | Store a model-namespaced vector; writer          |
| POST       | `/v1/search/vector`           | Exact scoped cosine search; reader               |
| GET / POST | `/v1/admin/keys`              | List key metadata / issue a scoped expiring key  |
| DELETE     | `/v1/admin/keys/{id}`         | Revoke a managed key                             |
| PUT        | `/v1/admin/principals`        | Provision or disable an OIDC subject             |
| GET        | `/v1/admin/audit/export`      | Bounded NDJSON export page                       |
| POST       | `/v1/admin/audit/expire`      | Expire audit history by age                      |
| POST       | `/v1/admin/encryption/rotate` | Re-encrypt a bounded batch per payload table     |

All `/v1/admin` routes require admin role in the current scope. See [service controls](usage/service-controls.md)
for request bodies, limits and operating procedures. Native gRPC uses the standard TraceService Export
route on the same port. It does not accept execution `.rfr` files.

## Project embeddings and text search

| Method | Path                           | Purpose                                                                            |
| ------ | ------------------------------ | ---------------------------------------------------------------------------------- |
| GET    | `/v1/auth/me`                  | Current authenticated identity, role, and scope                                    |
| GET    | `/v1/embedding-models`         | Operator-approved model profiles available to this scope; no credentials/endpoints |
| GET    | `/v1/project/embeddings`       | Enabled project profiles and indexing job counts                                   |
| PUT    | `/v1/admin/project/embeddings` | Admin replaces selections with an array of `{profile,is_default,auto_index}`       |
| POST   | `/v1/admin/embeddings/reindex` | Admin queues/retries indexing of recorded runs                                     |
| POST   | `/v1/search/text`              | Reader submits `{query,profile?,limit?,mode?}`; returns runs and scored matches    |

Text and vector search accept `mode: "auto"` (default), `"exact"`, or `"approximate"`.
Read the [search guide](usage/search.md) for provider profiles, preprocessing, namespaces,
worker recovery, cache sizing, and SDK examples. Text search may call the selected provider; vector
search only uses submitted/stored vectors.

## Generation models and executable branches

- `GET /v1/generation-models`: scoped, public profile metadata; no endpoints/credentials.
- `POST /v1/runs/{id}/rerun`: writer creates a stored model branch with
  `{profile,from_event,allow_live,approved_events:[],reuse_recorded:[]}`.
- `POST /v1/diff` and `/v1/eval`: optional `grader` profile and `allow_live:true` enable configured
  domain grading. Failures produce failing reports, never implied equivalence.

See [model execution](usage/rerun.md) for profiles, consent, provider protocols and timeout/size budgets.

## Telemetry and identity lifecycle

- `POST /v1/logs`, `POST /v1/metrics`: OTLP JSON/protobuf, writer role; equivalent native gRPC services share the API port.
- `GET /v1/telemetry`: reader query with `kind`, optional log `trace_id`, `limit`, `offset`.
- `POST /v1/auth/start`, `/v1/auth/complete`, `/v1/auth/logout`: same-origin browser session lifecycle.
- `/scim/v2/Users`, `/scim/v2/Groups`: scoped admin SCIM collections; `GET`/`POST` on collections and `GET`/`PUT`/`PATCH`/`DELETE` on IDs.
- `GET /scim/v2/ServiceProviderConfig`: scoped admin discovery of supported SCIM operations.

See [telemetry](usage/otel.md#logs-and-metrics) and [identity configuration](usage/service-controls.md#browser-sso)
for payloads, session expiry, group mappings, encryption and production configuration.
