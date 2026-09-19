# Local REST API

Requests and responses use JSON except artifact downloads. Errors return `{"error":"message"}`
for application errors; malformed JSON/content-type errors use Axum's HTTP error responses.

| Method | Path                     | Behavior                                                                   |
| ------ | ------------------------ | -------------------------------------------------------------------------- |
| GET    | `/v1/health`             | Process health                                                             |
| GET    | `/v1/ready`              | SQLite connectivity                                                        |
| POST   | `/v1/runs`               | Validate/redact/insert a complete canonical snapshot; 201 or duplicate 409 |
| GET    | `/v1/runs`               | Latest 100 snapshots                                                       |
| GET    | `/v1/runs/{id}`          | Snapshot or 404                                                            |
| GET    | `/v1/runs/{id}/events`   | Ordered events                                                             |
| GET    | `/v1/runs/{id}/artifact` | Download checksummed `.rfr`                                                |
| POST   | `/v1/runs/{id}/replay`   | `{"mode":"exact"}`; recorded steps only                                    |
| POST   | `/v1/runs/{id}/fork`     | `{"from_event":"evt_2"}`; persist a prefix fork                            |
| POST   | `/v1/diff`               | `{"left":"run_a","right":"run_b"}`; first divergence and differences       |

Configuration: `REFRACT_BIND` (default `127.0.0.1:8000`), `REFRACT_DATABASE_URL`
(default `sqlite://refract.db`), `REFRACT_UI_DIR` (default `apps/viewer/dist`).
SQLx applies embedded SQLite migrations on startup. Do not point this version at PostgreSQL.
The API has no live-replay, dataset, authentication, gRPC, or OTLP endpoints yet.

## Custom HTTP client example

```bash
curl -fsS http://localhost:8000/v1/ready
curl -fsS -X POST http://localhost:8000/v1/runs \
  -H 'Content-Type: application/json' \
  --data-binary @tests/fixtures/simple-run/execution.json
curl -fsS http://localhost:8000/v1/runs/demo-1/events
curl -fsS -X POST http://localhost:8000/v1/runs/demo-1/replay \
  -H 'Content-Type: application/json' -d '{"mode":"exact"}'
mkdir -p .examples
curl --fail --output .examples/api-export.rfr http://localhost:8000/v1/runs/demo-1/artifact
```

The export uses `application/vnd.refract.rfr` and the readable UTF-8 artifact profile. Input to
`POST /v1/runs` is canonical JSON, not the artifact header/body encoding: decode/validate the file first.
Requests are limited to 16 MiB. Invalid application data returns 400, missing runs 404, and duplicate
IDs 409. This API has no authentication and should remain local until production controls exist.
