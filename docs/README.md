# Refract documentation

Refract is one execution model and Rust engine exposed through multiple interfaces. Choose the mode
that fits your application; a running server is optional for file recording and CLI workflows.

| Guide                             | Purpose                                                                             |
| --------------------------------- | ----------------------------------------------------------------------------------- |
| [Python](usage/python.md)         | Instrument sync/async code and send or save recordings                              |
| [TypeScript](usage/typescript.md) | Use the npm SDK in Node applications                                                |
| [Rust](usage/rust.md)             | Embed crates and run the Rust example                                               |
| [CLI](usage/cli.md)               | Operate on files offline                                                            |
| [API](api.md)                     | Integrate through HTTP                                                              |
| [Docker and UI](usage/docker.md)  | Run the engine/viewer as a service, including from application code                 |
| [Inspector](usage/inspector.md)   | Explore the browser workspace, event details and screenshots                        |
| [MCP](usage/mcp.md)               | Configure agent tools and optional writes                                           |
| [Skills](usage/skills.md)         | Add task-specific agent guidance                                                    |
| [CI Action](usage/ci.md)          | Detect semantic execution changes                                                   |
| [Search](usage/search.md)         | Filter executions, compare lexical matches and search application-generated vectors |
| [Providers](usage/providers.md)   | Configure OpenAI, Anthropic, Gemini, Azure, Vertex, Bedrock and custom models       |
| [Metrics](usage/metrics.md)       | Interpret token/cost/latency measurements and completeness                          |
| [Rerun](usage/rerun.md)           | Execute explicit continuations with model replacements and approvals                |
| [Evaluation](usage/evaluation.md) | Compare semantics and evaluate datasets with budgets/custom graders                 |
| [OpenTelemetry](usage/otel.md)    | Native HTTP/protobuf/gRPC trace ingestion and SDK bridges                           |
| [Artifacts](usage/artifacts.md)   | Read, validate, convert and integrate `.rfr`                                        |
| [Production](production.md)       | Understand deployment boundaries and recording tradeoffs                            |
| [Development](development.md)     | Install tools, run checks and build packages locally                                |
| [Repository map](repository.md)   | Understand where each responsibility lives                                          |
| [Migrations](migrations.md)       | Evolve SQLite/PostgreSQL storage using SQLx                                         |
| [Architecture](architecture.md)   | Understand engine and interface boundaries                                          |
| [Roadmap](roadmap.md)             | See implemented capabilities and their operating boundaries                         |

- [Provider extensions and checkpoint continuation](usage/advanced-integrations.md)
- [OIDC, managed keys, vector search and durable acceptance](usage/service-controls.md)

- [Price feeds and invoice reconciliation](usage/pricing.md)
- [Backup and recovery](usage/recovery.md)
