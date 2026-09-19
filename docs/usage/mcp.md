# MCP agent mode

`refract-mcp` is a stdio service built with the official MCP Python SDK. It calls the Rust API; it does
not duplicate the execution engine or run model/tool code. Install from [source](../development.md).

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

| Tool                    | Purpose                                                  |
| ----------------------- | -------------------------------------------------------- |
| `health`                | Check server readiness                                   |
| `search_runs`           | Filter latest 100 snapshots by name/ID/status            |
| `list_failed_runs`      | Find failed snapshots within that window                 |
| `inspect_run`           | Read metadata and events                                 |
| `inspect_event`         | Read one event                                           |
| `show_execution_graph`  | Return nodes and parent-child edges                      |
| `compare_runs`          | Compare ordered event semantics                          |
| `find_first_divergence` | Read the first changed position                          |
| `export_run`            | Return canonical data and artifact download path         |
| `replay_recorded`       | Return captured outputs, with BLOCKED policy enforcement |

The `refract://capabilities` resource reports the transport and restrictions.
By default the server exposes only read-only tools, including non-mutating recorded playback.
Set **`REFRACT_MCP_ALLOW_WRITES=1`** in the MCP server environment to additionally register:

- `import_run(execution)`: persist a snapshot through normal validation/redaction.
- `fork_run(run_id, from_event)`: persist a prefix branch; never execute new steps.

Host/client approval settings still apply. The opt-in is not authentication or multi-user authorization.
No tool accepts arbitrary filesystem destinations. `export_run` does not save a file on the host.

## Is the interface complete?

It covers the current engine's read, comparison and recorded-playback operations. Optional writes cover
native ingestion and forks. It does not implement future capabilities such as live replay, checkpoint
continuation, datasets, provider substitution, paginated history or OTLP ingestion. Those require engine
work before MCP tools can honestly expose them. Search currently covers only the latest 100 runs.

Run `uv run python examples/mcp/client.py` for a real protocol handshake/tool/resource example.
The test suite checks discovery through stdio and the registration boundary for write tools.
