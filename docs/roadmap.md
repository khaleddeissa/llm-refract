# Capability status and deployment boundaries

The following capabilities have concrete implementations in this checkout. Published packages/images
may lag these changes until a release is made. Verification commands and deployment checks are in
[production operation](production.md) and [development](development.md).

| Area                      | Implemented                                                                                                                        | Boundaries                                                                                                                |
| ------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| Automatic instrumentation | OpenAI/Anthropic/Azure/Gemini/Vertex/Bedrock and custom adapters in Python and Node; LangChain callbacks in Python; explicit spans | Opt-in; supported SDK surfaces are tested, not universal monkey-patching of all frameworks                                |
| Executable replay         | Rust/Python executors, CLI adapters, model/input bindings and LangGraph checkpoint continuation                                    | Application handlers and credentials must be supplied; no arbitrary process-memory restoration                            |
| Semantic comparison       | Offline heuristic, external/embedded grader interfaces, factual-number checks and metric budgets                                   | Offline grading is not a semantic model; choose a domain/model grader for nuanced meaning                                 |
| Observability             | Usage, cached usage, refreshable price catalogs, invoice reconciliation, latency, TTFT, coverage and before/after deltas           | Unknown usage/pricing stays unknown; price feeds require an approved source; invoice rows require explicit event mappings |
| Graph UI                  | Virtualized recorded-parent graph (100,000-event layout test), event selection, metrics and search                                 | Parent graph reflects emitted edges; it does not infer missing causal edges                                               |
| Ingestion                 | Bounded SDK queues, sampling, batching, Python/Node fsync acceptance and idempotent batches                                        | Optional disk spool, finite quotas; not an exactly-once distributed message broker                                        |
| Service controls          | Scoped static/managed keys, OIDC/PKCE login, keyring rotation, optional RLS, audit export/expiry and shared SQL quotas             | OIDC requires provisioned subjects and browser client configuration; RLS requires a restricted PostgreSQL role            |
| OpenTelemetry             | OTLP JSON/protobuf HTTP and gRPC ingestion, durable distributed assembly and SDK bridges                                           | Trace/span IDs must be propagated upstream; no metrics/log ingestion or inferred missing spans                            |
| Evaluation                | Versioned dataset manifests, per-case options, CLI/CI reports and scoped API/MCP pair evaluation                                   | Candidates must be fresh recordings; no hidden model execution inside comparison                                          |
| Search                    | Indexed structured filters, pagination, lexical similarity and exact scoped vector search                                          | Application-generated vectors; no embedding model picker/text-to-vector endpoint; 10,000-candidate limit                  |
| Storage and delivery      | SQLite/PostgreSQL, durable delivery outbox, configurable downstream sinks                                                          | Operate and monitor dependencies; deployment-specific failure/recovery testing remains necessary                          |

The listed workstreams have implementations with local/mock verification. Live credentials, cloud
identity, ingress and recovery objectives must be validated for each deployment. Outbox leases fence acknowledgements and order retention deletion after active PUT leases;
external delivery remains at least once, with no total order across unrelated runs. Arbitrary process-memory restoration and inference of missing causal edges are not possible
from an execution recording. [Provider extensions](usage/advanced-integrations.md) and
[shared service controls](usage/service-controls.md) document newly implemented interfaces.

Use [production operation](production.md) before sharing a deployment, and [development](development.md)
for the tests that verify each supported mode. Recorded artifacts never authorize code execution or
choose an executable on behalf of a user.

Additional SDK surfaces can be added through custom adapters. Named adapters cover documented methods,
not every historical SDK, arbitrary inference runtime or framework operation. Approximate search beyond
10,000 vectors, SCIM/group synchronization, persistent browser refresh sessions and provider-specific
invoice importers are future extensions, rather than implied features of these interfaces.
