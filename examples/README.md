# Runnable examples by interface

Run from the repository root after [installing/building the relevant interface](../docs/development.md).
Recordings are created under `.examples/`, never in the repository root. Existing files are not overwritten.
The small readable reference recording in `artifacts/demo.rfr` is committed; generated output is ignored.

| Mode                  | Command / location                                                         | Demonstrates                             |
| --------------------- | -------------------------------------------------------------------------- | ---------------------------------------- |
| Python basic          | `uv run python examples/python/basic/record.py`                            | Parent-child events and local file       |
| Python RAG            | `uv run python examples/python/rag/record.py`                              | Documents and citations                  |
| Python state          | `uv run python examples/python/state/record.py`                            | State transition and blocked checkpoint  |
| Python failure        | `uv run python examples/python/failure/record.py`                          | Error recording and redaction            |
| TypeScript basic      | `node examples/typescript/basic/record.mjs`                                | Node SDK artifact export                 |
| TypeScript concurrent | `node examples/typescript/concurrent/record.mjs`                           | Isolated async runs                      |
| Rust                  | `cargo run -p refract-artifact --example inspect`                          | Direct engine embedding and round trip   |
| HTTP / Docker         | `python3 examples/http/ingest.py tests/fixtures/simple-run/execution.json` | Native snapshot upload                   |
| SDK → Docker API      | `uv run python examples/python/remote/record.py`                           | In-code use of the containerized service |
| MCP                   | `uv run python examples/mcp/client.py`                                     | Protocol initialization and discovery    |
| CLI / file            | `refract inspect examples/artifacts/demo.rfr`                              | Offline readable recording               |
| GitHub CI             | `examples/ci/regression.yml`                                               | Fresh-vs-baseline comparison             |
| Skills                | [agent request examples](../docs/usage/skills.md)                          | Evidence-based debugging                 |

The basic demo outputs are synthetic and need no provider keys. Provider examples below explicitly
call your configured endpoint or local weights where indicated. Production use requires your actual
application calls and appropriate capture policies; see the [deployment guide](../docs/production.md).

The [saved smoke-test artifacts](artifacts/smoke_test/README.md) include nine Python/Node outputs and a
regression report, with the scenario, value provenance and reproduction commands for each.

New experiment examples: [dataset evaluation](evaluation/README.md),
[executable rerun](rerun/README.md), and [automatic TypeScript capture](typescript/instrumented/README.md).

## Providers and observability

| Example                                                             | Purpose                                                     | Requirements                                       |
| ------------------------------------------------------------------- | ----------------------------------------------------------- | -------------------------------------------------- |
| [Custom Python model](python/providers/custom.py)                   | Adapt an ordinary method and save a recording               | Offline, no provider                               |
| [OpenAI-compatible endpoint](python/providers/openai_compatible.py) | Record Ollama, vLLM or a private gateway                    | Existing endpoint/model and optional `LLM_API_KEY` |
| [Local Transformers](python/providers/transformers_local.py)        | Record inference from existing local weights                | Transformers/backend installed; downloads disabled |
| [LangChain](python/providers/langchain.py)                          | Record a runnable through callbacks                         | `langchain-core`; offline                          |
| [Python Langfuse export](python/providers/langfuse_export.py)       | Preview OTLP JSON or explicitly export an artifact          | Keys/base URL only with `--send`                   |
| [Node providers](typescript/providers/README.md)                    | Provider shapes, custom/local inference and Langfuse export | See each command's requirements                    |

See [provider coverage](../docs/usage/providers.md) and [OpenTelemetry](../docs/usage/otel.md) for
supported methods, export boundaries and production configuration. Live provider examples may incur
the provider's normal charges; test suites use intercepted HTTP or deterministic fixtures.
