---
name: refract-doctor
description: Diagnose local Refract CLI, API and container availability.
---

# refract-doctor

Run from the repository root. Use `cargo run -p refract-cli --` in place of `refract` when the CLI is not installed.

Run `refract doctor`, GET /v1/health and GET /v1/ready. Inspect `docker compose ps` and `docker compose logs refract` for startup failures. SQLite/PostgreSQL migrations run automatically. In production mode, missing auth, encryption or explicit TLS-proxy configuration prevents startup; inspect configuration names without printing secrets. Report transport/storage capabilities accurately; do not reset the volume or delete recordings to repair a health failure.
