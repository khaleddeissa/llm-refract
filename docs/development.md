# Local development and installation

`llm-refract` (PyPI), `@llm-refract/sdk` (npm) and `ghcr.io/khaleddeissa/llm-refract` (container images) are
published on tagged releases. This page covers building from a source checkout instead, which is only
necessary for contributing to the project itself.

Requirements: Rust 1.94 (edition 2024), Python 3.11+ (`.python-version` selects 3.12 for workspace work),
`uv`, Node.js 22.12+ (24 recommended), npm, and Docker for container tests.

```bash
git clone https://github.com/khaleddeissa/llm-refract.git
cd llm-refract
make setup
make build
make test
make lint
make generate docs
```

`uv sync --locked --all-packages --all-extras` installs the Python workspace, optional provider SDKs
and development tools for complete contract testing. Runtime consumers install only their required extras.
To use the published packages in another project instead, use `uv tool install llm-refract` / `pip install llm-refract` / `pip install "llm-refract[mcp]"` or `npm install @llm-refract/sdk`.

```bash
cargo run -p refract-cli -- serve
# In another terminal; Vite proxies /v1 to the server:
npm run dev --workspace apps/viewer
# Alternatively, build the combined server/viewer image:
docker compose up --build -d --wait
```

## Checks by interface

| Check                                        | Command                                                                  |
| -------------------------------------------- | ------------------------------------------------------------------------ |
| Rust engine, storage and CLI                 | `cargo test --workspace --locked`                                        |
| Python SDK, MCP protocol, Action and schemas | `uv run pytest`                                                          |
| Python type checking                         | `uv run mypy`                                                            |
| TypeScript SDK/viewer                        | `npm test && npm run typecheck`                                          |
| Artifact interoperability                    | `make test-contract`                                                     |
| Running service                              | `make test-integration`                                                  |
| Browser workflow                             | `npx playwright install --with-deps chromium && make test-e2e`           |
| Skills                                       | Validate each `SKILL.md` frontmatter and exercise its referenced command |
| Formatting/lint                              | `make lint`                                                              |

MCP tests launch a real stdio subprocess; restricted execution sandboxes may require permission.
Example outputs are exclusively created under `.examples/`; use new paths or remove your own old
example output before rerunning. Checked-in fixtures are never overwritten by example commands.

## Python conventions

Root `pyproject.toml` is a non-distributable uv workspace. Shared development dependencies use
`[dependency-groups]`; runtime dependencies belong to each package. Member manifests declare
metadata, URLs, license files, explicit wheel package roots and typing markers. Ruff handles formatting,
import sorting and linting without competing Black/isort configuration. Mypy checks the SDK and MCP
with gradual typing; the existing decorator APIs are not advertised as fully strict-typed.
The MCP `<2` bound protects the FastMCP v1 API; it is an intentional compatibility constraint.
Lockfiles pin the tested environment while manifests express supported dependency ranges.

## Packaging

```bash
uv build --package llm-refract
npm pack -w @llm-refract/sdk
cargo build --release -p refract-cli -p refract-server
docker build -t llm-refract:local .
```

`make hooks` installs pre-commit and commit-message checks. GitHub workflows run on pushes, pull
requests or manual dispatch as applicable, never on cron. Dependabot checks monthly.
