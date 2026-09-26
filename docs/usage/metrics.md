# Tokens, cost and latency

Provider adapters record numeric measurements under `event.attributes` without changing the v1 run
schema. Manual/native integrations can emit the same fields:

| Field                                     | Meaning                                                                        |
| ----------------------------------------- | ------------------------------------------------------------------------------ |
| `provider`, `model`                       | Provider and model identifiers                                                 |
| `input_tokens`, `output_tokens`           | Provider-reported token usage                                                  |
| `cache_read_tokens`, `cache_write_tokens` | Provider-reported cached token usage when available                            |
| `total_tokens`                            | Provider-aware total, including separately billed cache usage where applicable |
| `cost_usd`                                | Cost computed from explicitly supplied prices; absent when unknown             |
| `ttft_ms`                                 | Time to first text/output token observed by streaming instrumentation          |

`duration_ms` remains the event duration. The SDKs never invent usage from string length or fetch
unversioned pricing behind the application's back. Configure rates in provider instrumentation; cost
is an estimate under that pricing policy, not an invoice. Cached pricing requires the appropriate
provider rate configuration.

Use `refract metrics run.rfr`, `GET /v1/runs/{id}/metrics`, MCP `run_metrics`, or the viewer. Reports
include model/tool counts, failures, measured token totals, priced/measured model-call coverage, total
cost, wall time, summed event durations, mean TTFT, and most expensive/slowest event IDs.

Partial captures are visible as coverage counts. Wall time is end minus start and is different from
the sum of event durations when work overlaps. Cost/usage comparisons become unknown when model-call
coverage is incomplete. These numeric metric fields survive key-based secret redaction; credential
fields such as `access_token` remain redacted.

See [evaluation](evaluation.md) for before/after deltas and configurable regression budgets.
