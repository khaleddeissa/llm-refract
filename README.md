<div align="center">

<img src="assets/llm-refract-logo.svg" width="96" height="96" alt="Refract green diamond" />

# llm-refract

**An open execution layer for AI systems.**

Record executions as portable artifacts. Inspect every step. Replay captured outputs. Fork and compare runs.

[![CI](https://img.shields.io/github/actions/workflow/status/khaleddeissa/llm-refract/ci.yml?branch=main&style=for-the-badge&label=CI)](https://github.com/khaleddeissa/llm-refract/actions/workflows/ci.yml)
[![Security](https://img.shields.io/badge/Security-CodeQL%20%26%20Dependency%20Review-2EA44F?style=for-the-badge&logo=github&logoColor=white)](https://github.com/khaleddeissa/llm-refract/actions/workflows/security.yml)
[![Latest release](https://img.shields.io/github/v/release/khaleddeissa/llm-refract?display_name=tag&sort=semver&style=for-the-badge)](https://github.com/khaleddeissa/llm-refract/releases)
[![Python](https://img.shields.io/badge/Python-3.11%2B-3776AB?style=for-the-badge&logo=python&logoColor=white)](https://www.python.org/downloads/)
[![Rust](https://img.shields.io/badge/Rust-2024-000000?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)
[![TypeScript](https://img.shields.io/badge/TypeScript-SDK-3178C6?style=for-the-badge&logo=typescript&logoColor=white)](packages/typescript)
[![MCP](https://img.shields.io/badge/Model%20Context%20Protocol-MCP-000000?style=for-the-badge)](packages/mcp)
[![Docker](https://img.shields.io/badge/Docker-Compose-2496ED?style=for-the-badge&logo=docker&logoColor=white)](docker-compose.yml)
[![License](https://img.shields.io/badge/License-Apache--2.0-0D75B8?style=for-the-badge)](LICENSE)

[Usage modes](#one-engine-many-interfaces) · [Examples](examples) · [API](docs/api.md) · [MCP](packages/mcp) · [Contributing](CONTRIBUTING.md)

</div>

## AI executions you can inspect, share and compare

`llm-refract` records model calls, tools, retrieval, decisions, state changes, checkpoints and failures
in a provider-neutral execution model. A portable `.rfr` recording connects your application,
terminal, browser, coding agent and regression pipeline.

```text
Python / TypeScript / native JSON
               ↓
      Canonical execution → readable .rfr
               ↓                 ↓
        Rust REST API       Rust libraries / CLI
               ↓                 ↓
       Web UI / MCP      inspect · replay · fork · diff
                                 ↓
                         regression checks in CI
```

## One engine, many interfaces

| Interface            | What you can do                                                                         | Guide                                  |
| -------------------- | --------------------------------------------------------------------------------------- | -------------------------------------- |
| Python SDK           | Record sync/async applications, decorators, explicit events, files and API submission   | [Python](docs/usage/python.md)         |
| npm / TypeScript SDK | Record Node applications, isolate concurrent runs, write/read artifacts, send snapshots | [TypeScript](docs/usage/typescript.md) |
| Rust crates          | Embed validation, artifacts, replay, diff and storage in your own Rust program          | [Rust](docs/usage/rust.md)             |
| CLI                  | Inspect, validate, pack/unpack, replay, fork, diff or start a server                    | [CLI](docs/usage/cli.md)               |
| REST API             | Store/read executions, export files, compare runs and create branches                   | [API](docs/api.md)                     |
| Docker image + UI    | Run a persistent local execution workspace and compare recordings in the browser        | [Docker](docs/usage/docker.md)         |
| MCP                  | Give an agent ten read-only tools; optionally enable import and prefix-fork tools       | [MCP](docs/usage/mcp.md)               |
| Skills               | Teach agents supported inspection, debugging, artifact and regression workflows         | [Skills](docs/usage/skills.md)         |
| GitHub Action        | Compare fresh application output against a reviewed execution baseline                  | [CI](docs/usage/ci.md)                 |
| `.rfr` format        | Carry readable, versioned, checksummed execution data between these interfaces          | [File format](docs/usage/artifacts.md) |

The Rust engine owns validation, persistence, replay policies and comparison. SDKs capture data;
MCP and the UI call the same API. Skills provide operating instructions; they are not separate engines.

## Record in your application

**Python**

```python
import refract

with refract.run("support-agent", path="support.rfr", endpoint="http://localhost:8000"):
    lookup = refract.event(type="retrieval", name="Find policy", output={"days": 30})
    refract.event(
        type="generation",
        name="Answer",
        parent_id=lookup,
        output={"text": "Returns are accepted within 30 days."},
        attributes={"provider": "custom", "model": "your-model"},
    )
```

**TypeScript / Node**

```typescript
import { refract } from "@refract-ai/sdk";

await refract.run(
  "support-agent",
  async () => {
    refract.event({
      type: "tool.call",
      name: "lookup_order",
      output: { status: "shipped" },
    });
  },
  { path: "support.rfr", endpoint: "http://localhost:8000" },
);
```

`endpoint` is optional: files work offline. Any provider or framework can emit canonical events;
automatic instrumentation for every provider is not implied.

```bash
pip install refract
uv tool install refract
npm install @refract-ai/sdk
docker pull ghcr.io/khaleddeissa/llm-refract:latest
```

See [installation and development](docs/development.md) for building from source instead.

## Debug locally, inspect in Docker, compare in CI

- **Offline development:** capture `.rfr`, open it in a text editor, then use the CLI to validate,
  inspect or compare it. No account, model key or server is required.
- **Application + Docker:** point either SDK at your container's REST endpoint. Open the bundled
  viewer to inspect a timeline, compare runs, fork before a step or export evidence.
- **Agent workflows:** attach the MCP stdio server and load an operational skill. Read tools inspect
  existing evidence; optional write tools import snapshots or create prefix branches.
- **GitHub CI:** generate an actual recording from the application under test and compare it against
  a reviewed baseline with the included repository Action.
- **Production capture:** manual recording can be integrated into application code, subject to your
  privacy/error-handling requirements. The current server lacks authentication, tenancy, retention
  and encrypted exports; it is not ready for public multi-tenant production hosting.
  See [production boundaries](docs/production.md).

## The `.rfr` execution file

New `.rfr` files are **UTF-8 text**: a one-line JSON format/checksum header followed by formatted
execution JSON. They open normally in editors. SHA-256 detects payload corruption; it is not a signature.

```bash
refract inspect recording.rfr
refract validate recording.rfr
refract unpack recording.rfr -o execution.json
refract fork recording.rfr --from evt_2 -o branch.rfr
refract diff recording.rfr branch.rfr
```

Older recordings used ZIP and showed binary characters in editors. The Rust reader still supports
them; [conversion instructions](docs/usage/artifacts.md) explain how to create a readable replacement.
Checked-in samples live in [examples/artifacts](examples/artifacts); generated examples go in `.examples/`.

Recorded replay returns captured outputs and never invokes external tools/models. Forking preserves
a prefix and lineage; it does not resume application code. Diff compares ordered event semantics,
not arbitrary application behavior. Regression checks need a **fresh** execution from the code under test.

## Execution inspector

Explore recorded events, inspect inputs and outputs, replay captured results, and compare runs in the
bundled browser workspace. See the [inspector walkthrough](docs/usage/inspector.md) for all three views.

[![Execution inspector with a selected generation event](assets/Inspector_Layout_3.PNG)](docs/usage/inspector.md)

## Explore the project

- [Documentation index and supported modes](docs/README.md)
- [Runnable examples in Python, TypeScript, Rust, HTTP, MCP and CI](examples/README.md)
- [Repository structure and the purpose of each directory](docs/repository.md)
- [Local development, tests and package builds](docs/development.md)
- [Database migrations](docs/migrations.md)
- [Current capabilities and remaining roadmap](docs/roadmap.md)
- [Contributing](CONTRIBUTING.md) · [Security](SECURITY.md) · [Apache-2.0 license](LICENSE)
