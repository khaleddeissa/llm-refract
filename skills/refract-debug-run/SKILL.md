---
name: refract-debug-run
description: Debug a Refract recording using inspect, recorded replay and semantic comparison.
---

# refract-debug-run

Run from the repository root. Use `cargo run -p refract-cli --` in place of `refract` when the CLI is not installed.

Validate and inspect the artifact first. Locate the earliest failed or surprising event and inspect its parents/input/output. Use recorded playback to examine captured outputs. If comparing a fix, obtain a fresh recording and diff it against the baseline. Prefix forks do not reexecute code. Report observed evidence and the limits of reproduction separately.
