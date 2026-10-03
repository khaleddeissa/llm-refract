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

## Built-in Python provider executors

Register an authenticated client once; Refract normalizes the recorded request, invokes the SDK,
awaits async clients, and records fresh usage. Sync SDK calls run off the async event loop.

```python
from openai import OpenAI
from refract.artifact import unpack
from refract.rerun import ExecutorRegistry, rerun
from pathlib import Path

source = unpack(Path("original.rfr").read_bytes())
executors = ExecutorRegistry()
executors.register_provider(OpenAI(), provider="openai", api="chat")
executors.reuse_recorded("retrieval", "decision")  # explicit reuse; no retrieval/tool side effects
branch = rerun(
    source,
    executors,
    from_event=source["events"][0]["id"],
    model="your-candidate-model",
    allow_live=True,
)
```

Built-ins accept OpenAI/Azure Chat and Responses, Anthropic Messages, Gemini/Vertex through
`google-genai`, Bedrock Converse, Ollama chat/generate, and LiteLLM completion/Responses clients.
`defaults` supplies trusted SDK options, such as Anthropic `max_tokens`. Recorded endpoints, keys,
headers and callbacks cannot override client configuration. Register application functions for tools
and nonstandard APIs; use [LangGraph checkpoints](advanced-integrations.md) for framework continuation.
Reusing an event does not bypass `BLOCKED` or explicit replay approvals.

## Server, Inspector, SDK and MCP model reruns

Operators set `REFRACT_GENERATION_PROFILES` or `REFRACT_GENERATION_PROFILES_FILE` to a JSON array:

```json
[
  {
    "id": "candidate",
    "label": "Candidate model",
    "protocol": "openai_chat",
    "endpoint": "https://gateway.example.com/v1/chat/completions",
    "model": "your-model-id",
    "credential_env": "MODEL_API_KEY",
    "max_output_tokens": 2048,
    "scopes": [
      { "organization": "company", "project": "support", "environment": "dev" }
    ],
    "grading_rubric": "Preserve refund eligibility, amounts and dates. Penalize unsupported claims."
  }
]
```

`credential_env` also supports the corresponding `_FILE` secret. Discovery exposes labels, models,
protocols, output limits and grading availability, without endpoints or secret variable names.
An omitted/empty `scopes` array makes the profile available to every authenticated scope.
Supported HTTP protocols are `openai_chat`, `openai_responses`, `anthropic`, `gemini`, `ollama`, and
`custom`. Gemini endpoints include the model and `:generateContent`; Ollama uses `/api/chat`.
OpenAI-compatible gateways, including LiteLLM and Azure/Foundry v1, use the matching OpenAI protocol.
Set `auth_header` to `authorization` (default Bearer), `api-key`, `x-api-key` or `x-goog-api-key`.
Use application SDK executors for AWS-signed/native Bedrock calls and cloud credential discovery.

Custom HTTP profiles supply `request_template` with whole-value `$input` and `$model` placeholders,
plus `response_pointer` pointing to text in the provider JSON response. Templates and transports are
operator configuration, not recording content. Production requires HTTPS; HTTP is available locally.
Redirects and implicit retries are disabled. Each call has a 25-second timeout, a 1 MiB request/response
limit, and shares four provider-call slots per service process. Server reruns accept up to 32 generation
steps and a 120-second overall deadline. Configure ingress timeouts to exceed that deadline.

```bash
export REFRACT_SERVER_URL=http://localhost:8000
refract generation-models
refract rerun-models RUN_ID --from evt_answer --profile candidate --allow-live -o branch.rfr
refract compare-runs RUN_ID BRANCH_ID --grader candidate --allow-live
```

`POST /v1/runs/{id}/rerun` requires a writer and accepts
`{profile,from_event,allow_live,approved_events:[],reuse_recorded:[]}`. Every non-generation event in
the suffix must appear by ID in `reuse_recorded`; its output is copied without running its application
code. Approval policies still apply to the whole suffix. The branch is validated/redacted and stored
with a new ID, so normal search, export and delivery apply. A failed request can already have incurred
provider usage; retrying is an explicit new execution, not a safe automatic retry.

```python
from refract import RefractClient

client = RefractClient(api_key="your-writer-key")
print(client.generation_models())
branch = client.rerun("RUN_ID", profile="candidate", from_event="evt_answer", allow_live=True)
report = client.compare("RUN_ID", branch["id"], grader="candidate", allow_live=True)
```

```typescript
import { RefractClient } from "@llm-refract/sdk";
const client = new RefractClient(
  "http://localhost:8000",
  process.env.REFRACT_API_KEY,
);
const branch = await client.rerun("RUN_ID", {
  profile: "candidate",
  from_event: "evt_answer",
  allow_live: true,
});
const report = await client.compare("RUN_ID", branch.id, {
  grader: "candidate",
  allow_live: true,
});
```

The Inspector's **Rerun with a model** panel lists available profiles, displays the selected start step,
and requires provider-call consent plus explicit output-reuse/policy approvals. MCP requires both
`REFRACT_MCP_ALLOW_WRITES=1` and `REFRACT_MCP_ALLOW_LIVE=1` for `rerun_models`; see [MCP](mcp.md).
