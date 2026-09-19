<div align="center">

# llm-refract

**An open execution layer for AI systems.**

Record executions as portable artifacts. Inspect every step. Replay captured outputs. Fork and compare runs.

[![CI](https://img.shields.io/github/actions/workflow/status/khaleddeissa/llm-refract/ci.yml?branch=main&style=for-the-badge&label=CI)](https://github.com/khaleddeissa/llm-refract/actions/workflows/ci.yml)
[![Security](https://img.shields.io/badge/Security-CodeQL%20%26%20Dependency%20Review-2EA44F?style=for-the-badge&logo=github&logoColor=white)](https://github.com/khaleddeissa/llm-refract/actions/workflows/security.yml)
[![Latest release](https://img.shields.io/github/v/release/khaleddeissa/llm-refract?display_name=tag&sort=semver&style=for-the-badge)](https://github.com/khaleddeissa/llm-refract/releases)
[![Python](https://img.shields.io/badge/Python-3.11%2B-3776AB?style=for-the-badge&logo=python&logoColor=white)](https://www.python.org/downloads/)
[![Rust](https://img.shields.io/badge/Rust-2024-000000?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)
[![TypeScript](https://img.shields.io/badge/TypeScript-SDK-3178C6?style=for-the-badge&logo=typescript&logoColor=white)](packages/typescript)
[![MCP](https://img.shields.io/badge/Model%20Context%20Protocol-MCP-000000?style=for-the-badge)](mcp)
[![Docker](https://img.shields.io/badge/Docker-Compose-2496ED?style=for-the-badge&logo=docker&logoColor=white)](docker-compose.yaml)
[![License](https://img.shields.io/badge/License-Apache--2.0-0D75B8?style=for-the-badge)](LICENSE)

[Quick start](#quick-start) · [Examples](examples) · [API](docs/api.md) · [MCP](mcp) · [Contributing](CONTRIBUTING.md)

</div>

## Why Refract?

AI failures span model calls, tool results, retrieval and changing application state.
Refract captures those steps in one provider-neutral execution model and a portable `.rfr` file,
so a recording can travel from a running application to a developer machine or CI.

- **Portable evidence** — versioned ZIP artifacts with SHA-256 checksums and bounded validation.
- **Provider-neutral recording** — Python and TypeScript SDKs with explicit events and async context isolation.
- **Execution inspection** — Rust CLI, REST API, React timeline and event inspector.
- **Recorded playback** — examine captured outputs without calling external tools or models.
- **Fork and compare** — preserve execution prefixes and identify semantic differences.
- **Agent access** — eight read-only MCP tools and ten operational skills.
- **Regression checks** — compare fresh application recordings with reviewed baselines in CI.

## Quick start

```bash
git clone https://github.com/khaleddeissa/llm-refract.git
cd llm-refract
docker compose up --build -d --wait
curl -fsS -X POST http://localhost:8000/v1/runs \
  -H 'Content-Type: application/json' \
  --data-binary @tests/fixtures/simple-run/execution.json
```

Open **http://localhost:8000** to inspect the sample execution, replay its outputs, fork before a step,
compare runs or download an artifact. Duplicate imports return `409` to preserve immutable snapshots.
SQLite data persists in a named Docker volume.

## Python

```bash
uv sync --locked
uv run python examples/python/basic/record.py
```

```python
import refract

with refract.run("support-agent", path="support.rfr"):
    lookup = refract.event(
        type="retrieval",
        name="Find policy",
        output={"documents": [{"id": "returns", "days": 30}]},
    )
    refract.event(
        type="generation",
        name="Answer",
        parent_id=lookup,
        output={"text": "Returns are accepted within 30 days."},
        attributes={"provider": "custom", "model": "your-model"},
    )
```

Add `endpoint="http://localhost:8000"` to submit the completed run to the API.
Recordings use new output files; existing artifacts are never silently overwritten.

## TypeScript

```bash
npm ci
npm run build -w @refract-ai/sdk
node examples/typescript/basic/record.mjs
```

```typescript
import { refract } from "@refract-ai/sdk";

await refract.run(
  "support-agent",
  async () => {
    refract.event({
      type: "tool.call",
      name: "lookup_order",
      input: { order_id: "123" },
      output: { status: "shipped" },
    });
  },
  { path: "support.rfr", endpoint: "http://localhost:8000" },
);
```

SDKs are currently consumed from this workspace; package registry publication is not enabled.
See [all examples](examples) for RAG, failures, state snapshots, concurrent runs and MCP.

## CLI

```bash
cargo run -p refract-cli -- pack tests/fixtures/simple-run/execution.json -o original.rfr
cargo run -p refract-cli -- inspect original.rfr
cargo run -p refract-cli -- replay original.rfr
cargo run -p refract-cli -- fork original.rfr --from evt_2 -o branch.rfr
cargo run -p refract-cli -- diff original.rfr branch.rfr
cargo run -p refract-cli -- validate branch.rfr
```

`diff` returns a nonzero exit status when executions differ. Forks contain the recorded prefix before
the chosen event; they do not execute new steps. Recorded playback returns captured values, not a rerun
of arbitrary application code.

## MCP and agent skills

```bash
REFRACT_SERVER_URL=http://localhost:8000 uv run refract-mcp
uv run python examples/mcp/client.py
```

Connect an MCP client through stdio to search runs, inspect events and execution graphs, compare runs,
find the first divergence or export execution data. See [client configuration](mcp/README.md).
[Operational skills](skills) document the implemented debugging, replay, fork and regression workflows.

## Regression checks

Generate a fresh recording from your application, then compare it against a reviewed baseline:

```bash
cargo build -p refract-cli
python3 action/compare.py baseline.rfr actual.rfr --cli target/debug/refract
```

The check writes `refract-report.json` and fails on semantic differences or invalid artifacts.
A [repository GitHub Action](action) is included; it is not yet published to Marketplace.

## Project layout

| Location               | Implementation                                                                         |
| ---------------------- | -------------------------------------------------------------------------------------- |
| `crates/`              | Rust model, artifact engine, storage, replay, diff, collector boundary, server and CLI |
| `packages/python/`     | Python instrumentation SDK                                                             |
| `packages/typescript/` | TypeScript instrumentation SDK                                                         |
| `ui/`                  | React execution viewer                                                                 |
| `mcp/server/`          | Executable MCP stdio service                                                           |
| `skills/`              | Ten operational agent skills                                                           |
| `action/`              | Baseline comparison runner and GitHub Action                                           |
| `spec/`, `migrations/` | Wire schemas, artifact format and SQLite migrations                                    |

The Rust server implementation is [here](crates/refract-server/src/lib.rs).
`sh server/run.sh` starts it locally; [API documentation](docs/api.md) lists endpoints and configuration.

## Development

Requires Rust 1.94, Python 3.11+, `uv`, and Node.js 22.12+ (24 recommended).

```bash
make setup
make lint test build
make generate docs
make hooks
# With the server running:
make test-integration
npx playwright install --with-deps chromium
make test-e2e
```

## Status and boundaries

Refract is an early development release for local use. The implemented stack uses SQLite and native
JSON ingestion. OTLP/gRPC, PostgreSQL/S3, automatic provider adapters and approved live execution
remain [roadmap work](docs/roadmap.md). Authentication and encrypted exports are not implemented.
Key-based redaction is enabled, but free-text sensitive data still requires review before sharing.
See [security guidance](SECURITY.md) and the [artifact specification](spec/artifact/rfr-v1.md).

## Contributing and license

Contributions are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) for development checks and
[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) for community expectations.
Licensed under [Apache-2.0](LICENSE).
