---
name: refract-doctor
description: Diagnose local Refract CLI, API and container availability.
---

# refract-doctor

Run from the repository root. Use `cargo run -p refract-cli --` in place of `refract` when the CLI is not installed.

Run `refract doctor`, GET /v1/health and GET /v1/ready. Inspect `docker compose ps` and `docker compose logs refract` for startup failures. SQLite migrations run automatically. Report transport/storage capabilities accurately; do not reset the volume or delete recordings to repair a health failure.
