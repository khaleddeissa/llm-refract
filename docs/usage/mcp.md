# MCP agent mode

`refract-mcp` (installed via `pip install "llm-refract[mcp]"`) is a stdio service built with the official MCP Python SDK. It calls the Rust API; it does not duplicate the execution engine or run model/tool code. Install from [source](../development.md).

```json
{
  "mcpServers": {
    "refract": {
      "command": "uv",
      "args": ["--directory", "/path/to/llm-refract", "run", "refract-mcp"],
      "env": { "REFRACT_SERVER_URL": "http://localhost:8000" }
    }
  }
}
```

| Tool                    | Purpose                                                               |
| ----------------------- | --------------------------------------------------------------------- |
| `health`                | Check server readiness                                                |
| `search_runs`           | Server-side name/ID/status/model/tool/latency filters with pagination |
| `list_failed_runs`      | Query the first page of failed snapshots                              |
| `inspect_run`           | Read metadata and events                                              |
| `inspect_event`         | Read one event                                                        |
| `show_execution_graph`  | Return nodes and parent-child edges                                   |
| `compare_runs`          | Compare ordered event semantics                                       |
| `find_first_divergence` | Read the first changed position                                       |
| `export_run`            | Return canonical data and artifact download path                      |
| `run_metrics`           | Measured usage, cost, latency and coverage                            |
| `evaluate_runs`         | Grade named stored run pairs with semantic/budget options             |
| `similar_runs`          | Lexically rank related executions                                     |
| `replay_recorded`       | Return captured outputs, with BLOCKED policy enforcement              |

The `refract://capabilities` resource reports the transport and restrictions.
By default the server exposes only read-only tools, including non-mutating recorded playback.
Set **`REFRACT_MCP_ALLOW_WRITES=1`** in the MCP server environment to additionally register:

- `import_run(execution)`: persist a snapshot through normal validation/redaction.
- `fork_run(run_id, from_event)`: persist a prefix branch; never execute new steps.

Host/client approval settings still apply. The opt-in is not authentication or multi-user authorization.
No tool accepts arbitrary filesystem destinations. `export_run` does not save a file on the host.

## Authentication and experiments

Set `REFRACT_API_KEY` in the MCP process environment for a secured service. The bearer key determines
its organization/project/environment and role. `REFRACT_MCP_ALLOW_WRITES` only controls tool discovery;
the service still authorizes every request. Do not paste keys into tool arguments or recordings.

`compare_runs(left, right, semantic=True, threshold=0.85)` includes offline output grading and metric
changes. `evaluate_runs` accepts named `{name,left,right}` pairs and evaluation options. Search accepts
`model`, `tool`, `min_duration_ms`, `limit` and `offset`; it is no longer limited to filtering a local
100-run window. All queries remain within the key's scope.

There are thirteen read tools and two opt-in write tools. Executable rerun is available through trusted
SDK/CLI executors, while MCP intentionally remains an evidence/query interface and never executes
arbitrary commands from a tool argument or recording.
