# Database migrations

The database belongs to the Rust storage crate. SQLx is already the migration framework; a `.sql` file
is its normal versioned migration representation, not an ad-hoc SQL script. Alembic is intended for
Python/SQLAlchemy ownership and would introduce a second migration authority here.

Migrations live in `crates/refract-storage/migrations/`. `Store::open` runs the embedded SQLx migrator
before the service becomes ready. SQLx records applied versions/checksums in `_sqlx_migrations` and
rejects incompatible changes. `build.rs` explicitly watches the migration directory so new SQL files
are embedded when rebuilding. `.gitattributes` keeps SQL line endings stable.

```bash
cargo install sqlx-cli --no-default-features --features sqlite
export DATABASE_URL=sqlite://refract.db
cargo sqlx migrate info --source crates/refract-storage/migrations
cargo sqlx migrate add --source crates/refract-storage/migrations add_run_indexes
```

Edit the **new** migration, test against an empty database and an upgraded copy, then rebuild/deploy.
Do not edit or renumber `0001_initial.sql` after deployment: its identifier and bytes are already part
of existing databases' migration history. Later SQLx-generated timestamp names can coexist with it.
The storage test verifies repeat initialization and retained data on the same database.

The current schema is SQLite-specific. PostgreSQL would require its own supported adapter/migration
set; changing the URL is not sufficient. Back up before upgrading. Prefer additive forward migrations
and explicit recovery planning over automatic destructive rollback. The app never drops data to repair
a migration failure.

References: [SQLx embedded migrations](https://docs.rs/sqlx/latest/sqlx/macro.migrate.html),
[SQLx CLI](https://github.com/launchbadge/sqlx/tree/main/sqlx-cli).
