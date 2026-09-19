---
name: refract-export-artifact
description: Export or validate a portable checksummed Refract execution artifact.
---

# refract-export-artifact

Run from the repository root. Use `cargo run -p refract-cli --` in place of `refract` when the CLI is not installed.

For canonical JSON run `refract pack INPUT.json -o NEW.rfr`; for a stored run download GET /v1/runs/{id}/artifact. Validate with `refract validate NEW.rfr`. MCP export_run returns a snapshot and download path rather than writing a file. Key-based redaction does not scrub free-text secrets. Checksums prove byte integrity, not authenticity.
