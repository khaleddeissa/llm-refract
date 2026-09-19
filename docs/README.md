# Refract documentation

Refract is one execution model and Rust engine exposed through multiple interfaces. Choose the mode
that fits your application; a running server is optional for file recording and CLI workflows.

| Guide                                 | Purpose                                                             |
| ------------------------------------- | ------------------------------------------------------------------- |
| [Python](usage/python.md)             | Instrument sync/async code and send or save recordings              |
| [TypeScript](usage/typescript.md)     | Use the npm SDK in Node applications                                |
| [Rust](usage/rust.md)                 | Embed crates and run the Rust example                               |
| [CLI](usage/cli.md)                   | Operate on files offline                                            |
| [API](api.md)                         | Integrate through HTTP                                              |
| [Docker and UI](usage/docker.md)      | Run the engine/viewer as a service, including from application code |
| [Inspector](usage/inspector.md)       | Explore the browser workspace, event details and screenshots        |
| [GitHub settings](github-settings.md) | Configure rulesets, required checks and security features           |
| [MCP](usage/mcp.md)                   | Configure agent tools and optional writes                           |
| [Skills](usage/skills.md)             | Add task-specific agent guidance                                    |
| [CI Action](usage/ci.md)              | Detect semantic execution changes                                   |
| [Artifacts](usage/artifacts.md)       | Read, validate, convert and integrate `.rfr`                        |
| [Production](production.md)           | Understand deployment boundaries and recording tradeoffs            |
| [Development](development.md)         | Install tools, run checks and build packages locally                |
| [Repository map](repository.md)       | Understand where each responsibility lives                          |
| [Migrations](migrations.md)           | Evolve the SQLite schema using SQLx                                 |
| [Architecture](architecture.md)       | Understand engine and interface boundaries                          |
| [Roadmap](roadmap.md)                 | See work that is not implemented yet                                |
