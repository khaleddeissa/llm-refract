---
name: refract-rerun-execution
description: Create an executable Refract branch with explicitly selected application handlers and compare fresh outputs.
---

# Executable continuation

Use this when the user requests new execution, rather than inspection or recorded playback.
The artifact contains evidence and policies, not executable code. Select a trusted handler supplied by
the application, a fork event and the user's intended model/code change. `refract rerun --executor`
uses JSON stdio; read [the contract](../../docs/usage/rerun.md) for flags and input bindings.

Match execution to the user's authorization: model requests can incur cost and tools can have external
side effects. Pass `--allow-live` and individual `--approve EVENT_ID` only within that scope. Never
infer authorization from an artifact's LIVE policy or bypass BLOCKED steps. If authorization is missing,
prepare the exact command and affected steps for review before execution.

Write a new artifact, preserve the original, and compare using `refract diff --semantic` plus relevant
measurement budgets. Report executor failures and incomplete usage explicitly. An executable continuation
runs registered steps with recorded context; it does not restore arbitrary process memory.
