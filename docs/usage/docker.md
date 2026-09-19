# Docker service and browser mode

The combined image contains the Rust server, CLI and built React viewer. It uses SQLite on `/data`,
runs as a non-root user, and exposes readiness at `/v1/ready`. It does not contain an inference model.
No public image has been published yet; build it as described in [development](../development.md).

```bash
docker compose up --build -d --wait
# Or, using an image you built:
docker run --rm -p 127.0.0.1:8000:8000 -v refract-data:/data llm-refract:local
```

Compose automatically discovers the root [`docker-compose.yml`](../../docker-compose.yml).
Both `.yml` and `.yaml` are supported extensions.

Open `http://localhost:8000`. Submit a run with an SDK or [HTTP example](../../examples/http/ingest.py),
select an event to inspect input/output/state, replay captured outputs, create a prefix fork, select
another run for comparison, or download `.rfr`. An empty workspace contains no fabricated telemetry.

## Use the container from application code

The application uses HTTP; it does not need access to the Docker socket.

```python
import refract

with refract.run("service", endpoint="http://localhost:8000"):
    refract.event(type="tool.call", name="lookup", output={"found": True})
```

TypeScript passes the same endpoint in `refract.run` options. From another container on the Compose
network, use `http://refract:8000`; `localhost` would point to that application container itself.
See [API reference](../api.md) for curl/custom-client integration.

```bash
docker compose exec refract refract doctor
docker compose logs refract
docker compose down
```

Compose preserves its named volume on shutdown. Do not use `down -v` unless intentionally deleting
recordings. Back up SQLite with a consistent SQLite backup or a stopped service. Database migrations
run on startup. See [production boundaries](../production.md) before exposing the service beyond localhost.

## Use the image as an offline CLI

```bash
docker run --rm -v "$PWD/examples/artifacts:/recordings:ro" llm-refract:local \
  refract inspect /recordings/demo.rfr
```

The command replaces the default server process. A read-only recording mount is enough for inspection;
commands that write new artifacts need a separate writable output mount with permissions for UID 10001.

See the [inspector walkthrough](inspector.md) for screenshots and explanations of each browser action.
