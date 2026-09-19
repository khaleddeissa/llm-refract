---
name: refract-replay-run
description: Replay captured Refract outputs without invoking providers or tools.
---

# refract-replay-run

Run from the repository root. Use `cargo run -p refract-cli --` in place of `refract` when the CLI is not installed.

Validate with `refract validate FILE.rfr`, then `refract replay FILE.rfr`. This returns captured outputs only. A BLOCKED event rejects playback. Do not claim application logic was rerun or treat LIVE/REQUIRES_APPROVAL metadata as permission to execute anything.
