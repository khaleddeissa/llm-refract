---
name: refract-fork-run
description: Create an unfinished Refract branch before a chosen event.
---

# refract-fork-run

Run from the repository root. Use `cargo run -p refract-cli --` in place of `refract` when the CLI is not installed.

Inspect the artifact to identify the exact event ID. Run `refract fork INPUT.rfr --from EVENT_ID -o NEW.rfr`, then inspect the new artifact. The branch contains the prefix before the event, preserves lineage and stays running. No continuation executor exists; do not claim downstream steps ran. Output paths must be new.
