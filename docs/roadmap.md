# Capability status and deployment boundaries

The following capabilities have concrete implementations in this checkout. Published packages/images
may lag these changes until a release is made.

| Area                      | Implemented                                                                                                                        | Boundaries                                                                                         |
| ------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| Automatic instrumentation | OpenAI/Anthropic/Azure/Gemini/Vertex/Bedrock and custom adapters in Python and Node; LangChain callbacks in Python; explicit spans | Opt-in; supported SDK surfaces are tested, not universal monkey-patching of all frameworks         |
| Executable replay         | Rust/Python executor interfaces, CLI command adapters, prefix lineage, model replacement and input bindings                        | Application handlers and credentials must be supplied; no arbitrary process-memory restoration     |
| Semantic comparison       | Offline heuristic, external/embedded grader interfaces, factual-number checks and metric budgets                                   | Offline grading is not a semantic model; choose a domain/model grader for nuanced meaning          |
| Observability             | Usage, cached usage, explicit pricing, latency, TTFT, coverage and before/after deltas                                             | Unknown usage/pricing stays unknown; no live price feed or invoice reconciliation                  |
| Graph UI                  | Interactive recorded-parent graph, event selection, metrics and search                                                             | Parent graph reflects emitted edges; it does not infer missing causal edges                        |
| Ingestion                 | Bounded background SDK queues, sampling, batching, retry spools and idempotent batches                                             | Optional disk spool, finite quotas; not an exactly-once distributed message broker                 |
| Service controls          | Scoped API keys, reader/writer/admin roles, audit, retention, rate limit, encrypted payloads                                       | TLS, secrets/key rotation, backups and identity-provider integration need deployment configuration |
| OpenTelemetry             | OTLP JSON conversion and Python SDK bridge; Node JSON bridge                                                                       | No native protobuf/gRPC collector or automatic collection from every agent framework               |
| Evaluation                | Versioned dataset manifests, per-case options, CLI/CI reports and scoped API/MCP pair evaluation                                   | Candidates must be fresh recordings; no hidden model execution inside comparison                   |
| Search                    | Indexed structured filters, pagination and scoped lexical similarity                                                               | Lexical similarity is not an embedding/vector index                                                |
| Storage and delivery      | SQLite/PostgreSQL, durable delivery outbox, configurable downstream sinks                                                          | Operate and monitor dependencies; deployment-specific failure/recovery testing remains necessary   |

Further work includes native OTLP protobuf/gRPC assembly, first-party OIDC login and account management,
vector search, automatic continuation of arbitrary framework processes, distributed workers and large-graph
layout optimization. These are separate capabilities, not implied by the APIs above.

Use [production operation](production.md) before sharing a deployment, and [development](development.md)
for the tests that verify each supported mode. Recorded artifacts never authorize code execution or
choose an executable on behalf of a user.
