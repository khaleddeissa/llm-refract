# Backup, upgrades and recovery

Back up the database, its encryption keyring, deployment configuration and any application-owned
LangGraph checkpoint store. Refract artifacts do not contain model credentials or checkpoint databases.
Keep backup copies encrypted with access/expiry policies independent of live run retention.

## SQLite

Use SQLite's backup API or `.backup`; copying only the main database file while WAL writes are active
can lose committed data. For a native deployment:

```python
import sqlite3

with sqlite3.connect("refract.db") as source, sqlite3.connect("private-backup.db") as backup:
    source.backup(backup)
```

Place the backup in private encrypted storage. Restore into a fresh file with the original keys, start
an isolated service, and compare run counts plus selected artifacts/metrics before redirecting clients.
For containers, perform the backup with a maintenance tool attached to the data volume or stop the
service and back up the complete volume. Preserve UID/GID permissions on restoration.

## PostgreSQL

Use version-compatible PostgreSQL tools with credentials supplied by your secret manager:

```bash
pg_dump --format=custom --file=private-backup.dump "$BACKUP_DATABASE_URL"
# RESTORE_DATABASE_URL must reference a NEW empty validation database, never your active one.
pg_restore --exit-on-error --no-owner --no-acl --dbname="$RESTORE_DATABASE_URL" private-backup.dump
```

Back up role/policy configuration separately. Reapply the restricted runtime role's grants and forced
RLS policies, and verify both application and database isolation before serving traffic. Encrypted
payloads require all referenced key IDs, including temporary trace spans and embeddings. Outbox rows
are restored too: isolate outbound delivery during restore validation to avoid replaying side effects.

## Upgrade and shutdown rehearsal

1. Snapshot the database and retain the old image/configuration and keyring.
2. Restore into a fresh validation database. Start the new image; SQLx verifies/applies migrations.
3. Check readiness, recorded artifacts, authorization, search and pending delivery counts.
4. Exercise your expected concurrency and payload sizes; measure latency and memory against your SLO.
5. Send SIGTERM and allow in-flight requests to finish. SDK applications must separately drain/close
   their exporters. Test a forced kill followed by restart to check persisted runs and private spools.
6. Verify the external destination's deduplication and retry behavior before enabling delivery.

`make test-recovery` creates only temporary local databases. Its fixed fixtures upgrade schema v1 to v5,
perform 32 writes with eight clients, back up active WAL data, restore into a fresh file, rotate keys,
query encrypted vectors, and exercise both SIGTERM and forced-kill restart. It is a correctness rehearsal,
not a throughput benchmark or production availability guarantee. PostgreSQL tests additionally exercise
actual migrations, transactions, quotas and restricted-role isolation; run both with a disposable database:

```bash
REFRACT_TEST_POSTGRES_URL=postgres://refract:password@localhost:5432/refract_test \
  cargo test -p refract-storage postgres_ -- --ignored --nocapture
```

Tests use local/mock services. Verify your actual TLS/DNS, provider auth, IdP, S3/webhook endpoint,
backup retention, recovery objectives and shutdown grace periods before rollout.
