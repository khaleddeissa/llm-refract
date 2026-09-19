---
name: refract-inspect-execution
description: Inspect a Refract artifact or stored run to understand recorded events.
---

# refract-inspect-execution

Run from the repository root. Use `cargo run -p refract-cli --` in place of `refract` when the CLI is not installed.

Use `refract inspect FILE.rfr` or MCP `inspect_run(run_id)`. For a particular event use `inspect_event`. Report status, inputs/outputs, parent relationships and relevant model attributes; distinguish missing data from recorded nulls.
