# Executable replay and branching

Recorded `refract replay` returns captured outputs and never runs external code. `refract rerun`
creates a new branch and invokes an executor you explicitly supply for each event from the selected
step onward. A portable recording describes evidence; it cannot serialize arbitrary application code,
credentials, closures or an entire process continuation.

```bash
refract rerun original.rfr --from evt_reason \
  --executor python3 --executor-arg my_executor.py \
  --model candidate-model --allow-live -o branch.rfr
refract rerun original.rfr --from evt_reason \
  --executor python3 --executor-arg my_executor.py \
  --replace-model old-model=new-model --approve evt_tool --allow-live -o branch.rfr
refract diff original.rfr branch.rfr --semantic
```

`--allow-live` explicitly authorizes execution. `BLOCKED` always refuses execution. Steps with
`REQUIRES_APPROVAL`, and tool calls/state changes/handoffs/human steps unless `READ_ONLY`, need their
individual `--approve` IDs. The entire suffix is checked before any executor is called. These policy
checks do not sandbox the executable; use only application-owned trusted executors. The server and
MCP never execute arbitrary command plugins.

The new branch preserves the prefix and original event IDs, assigns a new run ID, records lineage,
and substitutes the requested model in generation attributes/input. Usage and costs for rerun events
are cleared, so baseline measurements cannot be mistaken for new usage. The branch wall time starts
at continuation; retained prefix timestamps remain historical. Output files are never overwritten.

## Executor contract

The command receives one JSON request per event on stdin:

```json
{
  "event": {
    "name": "reason",
    "input": {},
    "attributes": { "model": "candidate-model" }
  },
  "context": { "events": [] }
}
```

Actual requests contain the complete canonical event and completed branch prefix. Return a JSON object
with `output` and optional `attributes`. Your code chooses providers/tools, computes updated inputs,
and emits usage measurements. Rust applications implement `refract_replay::Executor`; Python has an
executor registry and async runner. See [Python usage](python.md) and the
[executable local example](../../examples/rerun/README.md).

An event may declare `attributes.input_bindings` mapping input keys to prior event outputs:

```json
{
  "input_bindings": {
    "policy": { "event_id": "evt_retrieval", "path": "/documents" }
  }
}
```

The JSON pointer resolves against the source event's output in the completed branch prefix, including
freshly rerun outputs. Missing references fail execution. Branches do not automatically infer how one
step's text becomes another's prompt; encode that in bindings or in the executor.

Each CLI plugin invocation is bounded to 60 seconds and 1 MiB output. Failures stop the rerun; there is
no implicit retry of side effects. A failed external action may already have occurred, so applications
should use idempotency keys where appropriate. Rerunning from a checkpoint still requires executable
handlers and recorded state; it is not a process-memory restore.
