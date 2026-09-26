# Local and production operation

Refract records executions from any model provider because the engine stores canonical JSON events,
not provider credentials or SDK objects. Provider clients run in your application. Configure their
credentials, regional endpoints and model permissions there; configure the Refract service separately.
See [provider integrations](usage/providers.md) and [Python](usage/python.md)/[Node](usage/typescript.md).

## Choose an operating mode

| Mode           | Configuration                                                     | Intended use                                                                          |
| -------------- | ----------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| Offline        | SDK `output` or `.rfr` files and CLI                              | Local development, CI and restricted networks; no service required                    |
| Local service  | `docker compose up --build -d --wait`                             | Loopback-only Inspector/API with SQLite and a persistent volume                       |
| Shared service | `REFRACT_MODE=production`, scoped keys, encryption, HTTPS ingress | Authenticated applications and teams; PostgreSQL recommended for concurrent workloads |

Production mode refuses startup unless API keys, a valid encryption key and
`REFRACT_TLS_TERMINATED=1` are present. The TLS flag is the operator's assertion that a proxy terminates
HTTPS; the server itself speaks HTTP. Never publish its backend port directly. The deployment profile
below publishes only Caddy and puts Refract/PostgreSQL on an internal Docker network.

## Deploy the production profile

The [production Compose profile](../deploy/production/docker-compose.yml) includes the service,
PostgreSQL 17, and Caddy's HTTPS ingress. Provide a DNS hostname pointing at the host and permit ports
80/443 for certificate issuance and HTTPS. Docker Compose must have access to the secret files.

Generate initial secrets once from the repository root; this writes private files and prints no keys:

```bash
python3 - <<'PY'
import base64, json, os, secrets
from pathlib import Path
root = Path('deploy/production/secrets')
root.mkdir(mode=0o700, parents=True, exist_ok=True)
password = secrets.token_urlsafe(32)
keys = [dict(id=f'example-{role}', key=secrets.token_urlsafe(32), role=role,
             organization='example', project='assistant', environment='production')
        for role in ('reader', 'writer', 'admin')]
values = {
    'postgres_password': password,
    'database_url': f'postgres://refract:{password}@postgres:5432/refract',
    'encryption_key': base64.b64encode(secrets.token_bytes(32)).decode(),
    'api_keys.json': json.dumps(keys, indent=2),
}
for name, value in values.items():
    with (root / name).open('x') as stream:
        os.chmod(stream.name, 0o600)
        stream.write(value + '\n')
PY
export REFRACT_HOST=traces.example.com
# The runtime UID 10001 must be able to read its mounted secret files.
# On a native Linux host, assign only the three application secrets to its group:
sudo chgrp 10001 deploy/production/secrets/api_keys.json \
  deploy/production/secrets/database_url deploy/production/secrets/encryption_key
chmod 640 deploy/production/secrets/api_keys.json \
  deploy/production/secrets/database_url deploy/production/secrets/encryption_key
docker compose -f deploy/production/docker-compose.yml config --quiet
docker compose -f deploy/production/docker-compose.yml up --build -d --wait
curl --fail "https://${REFRACT_HOST}/v1/ready"
```

The generator deliberately refuses to overwrite existing files. Back up the secrets securely before
starting; database backups without the encryption key cannot recover event payloads. Secret mounts
use host file permissions, so adapt ownership for rootless Docker rather than making files public.
For a managed database, replace the database URL secret and remove the bundled PostgreSQL service;
use `sslmode=verify-full` and the database vendor's trusted certificate configuration.

Distribute a writer key to your recording application, a reader key to viewers, and an admin key only
to operators. The Inspector's key entry keeps the key in tab memory, without persistent browser
storage. Keys are SHA-256 hashed in server state and mapped to a fixed organization/project/environment.
Request headers cannot change that scope. Even an admin key administers only its own scope.

## Configuration reference

| Variable                           | Meaning/default                                                      |
| ---------------------------------- | -------------------------------------------------------------------- |
| `REFRACT_MODE`                     | `local` or `production`; default `local`                             |
| `REFRACT_BIND`                     | Native default `127.0.0.1:8000`; container listens on `0.0.0.0:8000` |
| `REFRACT_DATABASE_URL` / `_FILE`   | SQLite or PostgreSQL URL; default `sqlite://refract.db`              |
| `REFRACT_API_KEYS` / `_FILE`       | JSON array of scoped API keys; schema/example above                  |
| `REFRACT_REQUIRE_AUTH`             | `1` requires keys even in local mode                                 |
| `REFRACT_ENCRYPTION_KEY` / `_FILE` | Base64-encoded random 32-byte AES-256-GCM key                        |
| `REFRACT_TLS_TERMINATED`           | Must be `1` in production with HTTPS at ingress                      |
| `REFRACT_RATE_LIMIT`               | Positive requests/key/minute, default `600`; per server process      |
| `REFRACT_RETENTION_DAYS`           | Optional `1..36500`; hourly run cleanup using server receipt time    |
| `REFRACT_REDACT_KEYS`              | Additional comma-separated key fragments to remove recursively       |
| `REFRACT_REDACT_PATTERNS`          | JSON array of Rust regex patterns applied to text                    |
| `REFRACT_REDACT_EMAILS`            | `1` adds email redaction                                             |
| `REFRACT_UI_DIR`                   | Native default `apps/viewer/dist`; container `/app/ui`               |

For the secret-capable variables, use either the direct value or the corresponding `_FILE` variable;
setting both fails startup. Secret files are read at startup, so rotate API keys with an overlap window
and service restart. Encryption key rotation needs an explicit decrypt/re-encrypt migration; replacing
the key alone makes existing payloads unreadable and is rejected at startup.

AES-GCM protects the complete execution payload with the scope and run ID authenticated as associated
data. Searchable names, IDs, timestamps, model names, durations, costs and audit metadata remain visible
in database indexes. Use encrypted volumes/database backups for whole-database protection. Existing
plaintext payloads are converted when opening the database with encryption enabled; historical backups,
SQLite free pages/WAL and exported `.rfr` files are not retroactively encrypted.

## Delivery, retention and application reliability

Python `BackgroundExporter` and Node `BatchExporter` support bounded background queues, batches,
retries and optional private disk spools. Python also provides deterministic sampling and byte limits.
Call `close()`/`shutdown()` during graceful application shutdown; monitor exporter counters and failures.
These are retry spools, not guarantees against a crash before persistence. Direct synchronous exports
still propagate errors; choose the failure policy that suits your application. See the SDK guides.

The service can atomically enqueue persisted snapshots for external delivery:

| Variable                                                             | Purpose                                                         |
| -------------------------------------------------------------------- | --------------------------------------------------------------- |
| `REFRACT_WEBHOOK_URL`                                                | Optional webhook endpoint; HTTPS required in production         |
| `REFRACT_WEBHOOK_SECRET` / `_FILE`                                   | HMAC signing secret; production requires at least 32 bytes      |
| `REFRACT_S3_ENDPOINT`                                                | Optional S3-compatible path-style endpoint; HTTPS in production |
| `REFRACT_S3_BUCKET`, `REFRACT_S3_REGION`                             | Bucket and SigV4 region                                         |
| `REFRACT_S3_ACCESS_KEY` / `_FILE`, `REFRACT_S3_SECRET_KEY` / `_FILE` | Object-storage credentials                                      |

The supplied isolated production profile has no application egress. To enable delivery, attach Refract
to an explicitly controlled egress network and allow only the destination endpoints. S3 supports static
SigV4 credentials; temporary STS session tokens and workload-identity credential discovery are not yet
implemented. The external bucket must already exist and allow PUT/DELETE for the configured prefix.

Delivery uses a durable database outbox, 120-second leases, 30-second HTTP timeouts and capped retry
backoff. It is **at least once**: webhook consumers must deduplicate `x-refract-delivery-id` and validate
`x-refract-signature` (`sha256=` plus the HMAC-SHA256 hex digest of the exact request body). Envelopes
contain `id`, `scope`, `operation`, `run_id`, and the stored `payload` string. With encryption enabled,
that string remains encrypted; this is not an `.rfr` export. S3 stores the same payload at
`bucket/organization/project/environment/sha256(run_id).json`; DELETE jobs remove retained objects.
Do not assume total ordering across concurrent workers. Drain pending jobs before removing a target.

Monitor `/v1/ready`, authenticated `/v1/admin/outbox`, authenticated `/v1/admin/audit`, reverse-proxy
logs and exporter failures. Retention removes runs/events and queues object deletion; it does not expire
audit history or historical backups. Authenticated requests are audited after execution, including role
and rate-limit rejections. Invalid/absent credentials and health requests are not scoped audit entries;
retain ingress access logs for those events. An audit-write failure returns HTTP 500 even if a mutation
already committed; batch retries are content-checked and idempotent.

## Operational validation and remaining limits

Run `cargo test --workspace` for auth, scope isolation, encryption/tampering, retention, batch atomicity,
secret-file startup and mock HTTP delivery contracts. A disposable PostgreSQL instance can run the real
database contract:

```bash
export REFRACT_TEST_POSTGRES_URL=postgres://refract:password@localhost:5432/refract_test
cargo test -p refract-storage postgres_platform_contract -- --ignored --nocapture
```

CI supplies PostgreSQL for that test. The normal suite explicitly skips it without a database. Before
rollout, test backup restore and upgrades, TLS/DNS, your real S3/webhook destination, provider credentials,
traffic limits and shutdown behavior in your environment. These checks do not establish an uptime SLA
or compliance certification. Per-process rate limits need an ingress limit for a replicated service;
there is no SSO, user provisioning, key-management API, database row-level security or audit export/expiry
policy. Transport/database credentials and backup lifecycle remain deployment responsibilities.

The API performs recorded playback and prefix forks; it does not run arbitrary provider/tool code on the
service. Executable reruns use explicitly registered handlers in your application or CLI, with approvals
for declared side effects. See [rerun](usage/rerun.md) and [migration operations](migrations.md).
