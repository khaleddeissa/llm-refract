---
name: refract-investigate-failure
description: Investigate a failed Refract run from recorded evidence.
---

# refract-investigate-failure

Run from the repository root. Use `cargo run -p refract-cli --` in place of `refract` when the CLI is not installed.

Use MCP `list_failed_runs`, then `inspect_run` and `inspect_event` for the first failed event. Follow its parent IDs and inspect preceding state/retrieval inputs. Distinguish captured error type from a guessed root cause. Export using the artifact API when requested; review data before sharing.
