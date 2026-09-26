# Database migrations

The Rust storage crate owns the database. SQLx applies versioned SQL migrations; Alembic would introduce
a separate Python migration authority for the same schema. The current statements support both SQLite
and PostgreSQL through SQLx's Any adapter. SQLite uses one pooled connection; PostgreSQL uses up to ten.

Migrations live in `crates/refract-storage/migrations/`. `Store::open` runs the embedded migrator before
the server listens. SQLx records versions/checksums in `_sqlx_migrations` and rejects edited history.
`build.rs` watches the directory, and `.gitattributes` preserves stable SQL line endings.

- `0001_initial.sql`: the original immutable run snapshot table.
- `0002_platform.sql`: organization/project/environment scopes, receipt timestamps, search indexes,
  event indexes, audit history and durable delivery outbox. Existing data moves into
  `local/default/development`; startup backfills event indexes and derived metrics.

Before enabling retention on an upgraded database, inspect the imported data: legacy rows use their
original start time as the receipt-time fallback because no receipt timestamp previously existed.
New ingestion always uses server receipt time, preventing caller-supplied timestamps from controlling
retention. Existing local data is visible to an authenticated key only if it maps to that same scope.

```bash
cargo install sqlx-cli --no-default-features --features sqlite,postgres,rustls
export DATABASE_URL=sqlite://refract.db
cargo sqlx migrate info --source crates/refract-storage/migrations
cargo sqlx migrate add --source crates/refract-storage/migrations add_run_indexes
```

Edit the new migration and test against an empty database plus an upgraded copy of each supported
backend. Do not edit or renumber already-deployed migrations. Timestamped SQLx names can coexist with
the original numeric versions. Rebuild so migrations are embedded in the binaries, back up both the
database and encryption key, then deploy. Schema startup/backfill may take time for large databases;
perform the upgrade during a controlled maintenance window.

`cargo test -p refract-storage` verifies repeat initialization, preservation, scope isolation, batches,
search and encryption with SQLite. For the actual PostgreSQL backend, CI creates a disposable service:

```bash
export REFRACT_TEST_POSTGRES_URL=postgres://refract:password@localhost:5432/refract_test
cargo test -p refract-storage postgres_platform_contract -- --ignored --nocapture
```

The PostgreSQL contract is explicitly ignored in normal local runs; selecting it requires the URL.
Use a disposable database because it applies migrations and writes/deletes fixture data.

Prefer additive forward migrations and rehearsed recovery over automatic destructive rollback. The app
never drops a database to repair a migration failure. Encryption protects execution payloads; indexed
metadata remains queryable. Enabling encryption converts existing plaintext execution payloads during
startup but does not sanitize old backups/WAL pages. See [production operations](production.md).

References: [SQLx embedded migrations](https://docs.rs/sqlx/latest/sqlx/macro.migrate.html),
[SQLx CLI](https://github.com/launchbadge/sqlx/tree/main/sqlx-cli).
