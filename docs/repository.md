# Repository map

| Directory                         | Purpose                                                                 | Shipped functionality or examples?  |
| --------------------------------- | ----------------------------------------------------------------------- | ----------------------------------- |
| `crates/refract-core`             | Canonical events, validation, redaction                                 | Engine library                      |
| `crates/refract-artifact`         | Text writer, checksums, text/legacy-ZIP reader                          | Engine library                      |
| `crates/refract-replay`           | Recorded playback and prefix branching                                  | Engine library                      |
| `crates/refract-diff`             | Ordered semantic comparison                                             | Engine library                      |
| `crates/refract-storage`          | SQLite persistence and colocated SQLx migrations                        | Engine library                      |
| `crates/refract-collector`        | Native run normalization before persistence                             | Engine library, not an OTLP service |
| `crates/refract-server`           | REST handlers and static UI delivery                                    | Executable service                  |
| `crates/refract-cli`              | Offline artifact commands and server launcher                           | Executable CLI                      |
| `packages/python`                 | Installable Python SDK; manual adapter under `refract.integrations`     | Library                             |
| `packages/typescript`             | npm SDK for Node; public TypeScript types                               | Library                             |
| `packages/python/src/refract_mcp` | MCP stdio service, bundled in the `llm-refract` package (`[mcp]` extra) | Interface module package            |
| `packages/github-action`          | Composite GitHub Action and local comparison runner                     | Interface package                   |
| `apps/viewer`                     | React timeline, inspector, browser tests                                | UI application                      |
| `skills`                          | Discoverable operational `SKILL.md` instructions                        | Agent interface                     |
| `spec`                            | Language-neutral execution schemas, vocabulary, artifact specification  | Public contract                     |
| `examples`                        | Runnable demonstrations and small readable reference artifacts          | Examples                            |
| `.examples`                       | Generated local recordings, ignored by Git                              | Local output only                   |
| `tests/contract`                  | Cross-language fixture/schema compatibility                             | Tests                               |
| `tests/integration`               | Running API/container checks                                            | Tests                               |
| `tools/dev`                       | Documentation and contract maintenance commands                         | Developer tooling                   |
| `tools/benchmarks`                | Repeatable serialization measurements                                   | Developer tooling                   |
| `deploy`                          | Docker entrypoint and Compose startup helper                            | Deployment tooling                  |
| `.github`                         | Push/PR/manual workflows and monthly Dependabot                         | Repository automation               |
| `assets`                          | SVG branding and inspector screenshots referenced by documentation      | Documentation assets                |
| `docs`                            | Usage, operation and contributor guides                                 | Documentation                       |

Root manifests (`Cargo.toml`, `pyproject.toml`, `package.json`) coordinate workspaces and lockfiles.
The root `Dockerfile` and `docker-compose.yml` remain easy-to-find deployment entrypoints. `Makefile` orchestrates
language tools; it does not implement runtime behavior.

The former `proto/` directory contained an unused draft Protobuf envelope. No generated code, gRPC
service or active caller consumed it, so it and the empty protocol re-export crate were removed.
JSON Schemas in `spec/` define today's wire contract. A future working gRPC transport should introduce
its `.proto` files and generation tests together, rather than shipping unused transport scaffolding.

The former top-level collector uploader is now `examples/http/ingest.py`. The actual native collector
remains in Rust. The manual provider adapter belongs to the Python SDK and moved there. Tests,
examples, deployment scripts and maintenance tools now have separate homes.
