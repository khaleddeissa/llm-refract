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
(default `sqlite://refract.db`), `REFRACT_UI_DIR` (default `ui/dist`).
SQLx applies embedded SQLite migrations on startup. Do not point this version at PostgreSQL.
The API has no live-replay, dataset, authentication, gRPC, or OTLP endpoints yet.
