# Operational skills mode

Skills are instructions that help an agent use existing CLI/API/MCP operations correctly. They do not
provide a new runtime or bypass replay policies. Each directory under `skills/` contains a discoverable
`SKILL.md` with a name, description, supported commands and important limitations.

Available skills: inspect-execution, replay-run, fork-run, diff-runs, create-regression,
investigate-failure, query-traces, export-artifact, doctor and debug-run, all prefixed `refract-`.

Install/copy the selected skill directories into your coding agent's supported skills location, or load
their `SKILL.md` explicitly. Supply an artifact path or run/event IDs. Make the Rust CLI available
(or use its cargo invocation), and connect the MCP server if the chosen workflow uses API-backed tools.

Example request: “Use refract-debug-run to inspect `.examples/failure.rfr`; explain the earliest
recorded failure and compare it with `.examples/fixed.rfr`.”

A skill cannot reproduce missing state or execute an unsupported live replay. Recorded outputs,
observed errors and suspected causes must remain distinguishable in the agent's report.
