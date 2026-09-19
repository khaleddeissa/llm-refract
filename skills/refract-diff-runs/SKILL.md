---
name: refract-diff-runs
description: Compare captured Refract executions and identify their first semantic divergence.
---

# refract-diff-runs

Run from the repository root. Use `cargo run -p refract-cli --` in place of `refract` when the CLI is not installed.

Use `refract diff LEFT.rfr RIGHT.rfr` or MCP `find_first_divergence(left, right)`. CLI exit 1 may indicate differences or an error: require valid JSON diff output before reporting a regression. Diff aligns events by position and ignores generated IDs/timing. Explain the first changed input/output and affected following events without inferring unrecorded causality.
