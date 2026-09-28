# Shared service controls

The 0.1.4 source adds database-backed controls to the existing organization/project/environment scope.
Use the same PostgreSQL database for replicas. Local SQLite remains supported. These interfaces require
normal authentication and are included in request auditing.

## Shared quotas and vector search

`REFRACT_RATE_LIMIT` is a fixed-minute request quota per identity and scope, enforced by an atomic
SQL upsert. All replicas must share the database and have synchronized clocks. Database errors fail
closed. Ingress-level connection and unauthenticated-request limits are still useful.

Generate embeddings using an application-selected local model or provider, then attach a vector:

```http
PUT /v1/runs/RUN_ID/embedding
Authorization: Bearer WRITER_KEY
Content-Type: application/json

{"model":"local-embedding-v1","values":[0.4,0.9,0.1]}
```

```http
POST /v1/search/vector
Authorization: Bearer READER_KEY
Content-Type: application/json

{"embedding":{"model":"local-embedding-v1","values":[0.4,0.8,0.2]},"limit":20}
```

Results contain run IDs and exact cosine similarity scores. Model name and dimensions define a search
namespace; tenants never share candidates. Vectors have 1–4096 finite dimensions and cannot be zero.
Exact search supports 10,000 candidates per namespace and refuses larger sets rather than silently
omitting candidates. Result limits are 1–100. Use a distinct model/version name when embeddings change.
Embeddings use configured payload encryption and expire with their run. This is an exact vector store,
not an approximate-nearest-neighbor index for millions of vectors.

## Managed keys and OIDC

Bootstrap with a configured administrator API key. An administrator can issue a scoped key with
`POST /v1/admin/keys` and `{"role":"writer","expires_in_days":30}`. The response shows the secret once;
only a SHA-256 digest is stored. List metadata with `GET /v1/admin/keys`, and revoke immediately with
`DELETE /v1/admin/keys/KEY_ID`. Keys cannot exceed one year. Rotation means issue a replacement, update
clients, then revoke the old key. Key responses must be treated as secrets.

Configure `REFRACT_OIDC_ISSUER`, `REFRACT_OIDC_AUDIENCE` and `REFRACT_OIDC_JWKS_URL`. The JWKS URL must
use HTTPS. Access tokens require signature, issuer, audience, subject and expiry validation; future
`nbf` values are rejected. RS256, ES256 and EdDSA are accepted, and symmetric JWT algorithms are rejected.
Public signing keys refresh on a bounded cache. The issuer's token claims cannot grant tenant scope or
administrator access.

An administrator provisions the subject within their own scope:

```http
PUT /v1/admin/principals
Authorization: Bearer ADMIN_KEY
Content-Type: application/json

{"subject":"issuer-subject-id","role":"reader","enabled":true}
```

The same endpoint with `enabled:false` disables the subject. A different tenant's administrator cannot
take over an existing subject binding. Only provisioned, enabled subjects may authenticate.
Applications can supply bearer tokens directly or use the Inspector browser flow described below.

## Audit lifecycle

`GET /v1/admin/audit/export?limit=1000&offset=0` downloads a bounded NDJSON page. Continue pagination
for additional records; use an export window without concurrent expiry when archiving by offset.
`POST /v1/admin/audit/expire` with `{"days":90}` expires older audit entries in the administrator's scope,
independently of run retention. Archive first when records must be preserved. The expiry request is
itself audited. Failed bearer authentication emits a credential-free server log; configure ingress
logging for request origin and rate control. Backup expiry remains the storage operator's policy.

## Durable SDK acceptance and workers

Python `BackgroundExporter(..., spool_dir="/private/spool", durable=True)` synchronously fsyncs both
file contents and the directory rename before returning `True` from `submit`. It requires POSIX and a
dedicated directory on a persistent volume. This adds disk latency; the default remains a background
retry spool. Capacity errors return `False`, which applications can handle explicitly. Delivery is still
at least once: a crash after remote commit but before acknowledgement can resend a snapshot.

Worker acknowledgements carry a lease token, preventing an expired worker from acknowledging a job
claimed by another worker. Outbox creation remains atomic with run insertion. External consumers must
deduplicate, and cross-worker ordering is not an exactly-once guarantee.

S3 supports `REFRACT_S3_SESSION_TOKEN` with explicit temporary credentials. If explicit access/secret
keys are omitted, the AWS SDK credential chain discovers environment/profile, web-identity, ECS or
instance-role credentials. Do not combine one static key with discovery. Consult the AWS
[credential-provider guide](https://docs.aws.amazon.com/sdk-for-rust/latest/dg/credproviders.html).

## Browser SSO

The Inspector supports authorization code with S256 PKCE. In addition to the issuer/audience/JWKS
verification settings above, configure:

```bash
REFRACT_OIDC_CLIENT_ID=refract-inspector
REFRACT_OIDC_AUTHORIZATION_URL=https://identity.example.com/authorize
REFRACT_OIDC_TOKEN_URL=https://identity.example.com/token
REFRACT_OIDC_REDIRECT_URI=https://traces.example.com/
REFRACT_OIDC_SCOPES='openid profile'
# Optional provider routing parameters; only audience and resource are accepted:
REFRACT_OIDC_AUTH_PARAMS='{"audience":"refract-api"}'
```

Register a public browser client with the exact redirect URI and allow its origin at the token endpoint.
Do not configure a client secret in the browser. Tokens must be JWT access tokens for the configured API
audience, with a provisioned subject. `/v1/auth/config` exposes only public login configuration. The
Inspector checks state, issuer/client/redirect binding and a ten-minute callback expiry. PKCE state lives
in session storage until consumed; the access token lives only in tab memory. Reloading requires sign-in;
there is no persistent refresh token. Your identity provider owns MFA, session termination and user
lifecycle. Provisioning is an admin API, not an automatic SCIM/group synchronization service.

## Encryption rotation

Choose the original `REFRACT_ENCRYPTION_KEY` or a private `REFRACT_ENCRYPTION_KEYS_FILE`, never both.
The keyring format is `{"active":"next","keys":{"legacy":"BASE64_OLD_32_BYTES","next":"BASE64_NEW_32_BYTES"}}`.
`legacy` reads original `enc:v1:` payloads; new writes include and authenticate the active key ID.
Keep the old key while deploying the new ring to every replica, then set the same active key everywhere.

`POST /v1/admin/encryption/rotate` with `{"limit":100}` re-encrypts up to that number of rows **per payload
table** in the administrator's scope. Repeat until `rotated` is zero. Alternatively set
`REFRACT_ENCRYPTION_ROTATE_BATCH=100` for the background worker to rotate all scopes incrementally. It
covers recordings, embeddings and pending trace spans, and requeues downstream object updates. Startup
checks all represented key IDs and rejects missing or incorrect keys. Retain old keys until external
objects, exports and historical backups using them have expired or been migrated. Refract does not
rotate database passwords, ingress certificates or a cloud KMS key for you.

## PostgreSQL row-level security

Use a separate restricted runtime role in addition to the migration/maintenance connection:

1. Apply migrations using `REFRACT_DATABASE_URL` before enabling the restricted role.
2. Create an application login with `NOSUPERUSER NOBYPASSRLS`. Set its password through your secret manager.
3. As migration owner, apply [row-security.sql](../../deploy/production/row-security.sql) with
   `psql "$MIGRATION_DATABASE_URL" -v runtime_role=refract_runtime -f deploy/production/row-security.sql`.
4. Set `REFRACT_RUNTIME_DATABASE_URL_FILE` to that role's database URL; restart and check readiness.

The server refuses a superuser/BYPASSRLS runtime or missing forced RLS policies. Each pooled connection
gets organization/project/environment settings on checkout, with transaction-local settings inside
transactions. Eight tenant-data tables use `USING` and `WITH CHECK`; authentication registries remain
global because identifying a token precedes knowing its scope. Their admin operations still enforce
scope. Migration/worker credentials are privileged and must be protected separately. RLS guards omitted
application filters, not arbitrary SQL execution using a compromised application credential that can
change session settings. SQLite isolation remains application-enforced.

Node `BatchExporter` also supports `durable: true`, `spoolDirectory`, `maxQueueBytes` and `maxSpoolBytes`.
Await export acceptance and use `failOpen: false` if disk failure must fail the recording call. Keep one
exporter per private POSIX spool, and keep recovery queue limits at least as large as when it was written.
A forced kill after disk acceptance is recoverable on restart. No mode guarantees remote exactly-once
side effects; lease fencing protects acknowledgement, and retention waits for an in-flight PUT lease
before scheduling deletion. Do not assume global ordering across separate recordings or targets.

Audit exports use offset pagination over a live audit log. Export requests themselves add audit entries;
archive consumers must deduplicate entry IDs and account for concurrent inserts, or take a consistent
database snapshot for a strict point-in-time archive. The API is intended for bounded operational export.
