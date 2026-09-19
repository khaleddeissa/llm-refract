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

Demo model outputs are synthetic and need no provider keys. Production use requires your actual
application calls and appropriate capture policies; these examples are not production deployment templates.
