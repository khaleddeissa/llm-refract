# Refract MCP

The executable stdio server is in [server/src/refract_mcp/server.py](server/src/refract_mcp/server.py).
It uses the official MCP Python SDK and the Rust REST API.

```bash
uv sync --locked
REFRACT_SERVER_URL=http://localhost:8000 uv run refract-mcp
```

Tools: `search_runs`, `list_failed_runs`, `inspect_run`, `inspect_event`, `show_execution_graph`,
`compare_runs`, `find_first_divergence`, `export_run`. Resource: `refract://capabilities`.
All tools are read-only. Export returns data and a download path; it does not write arbitrary files.
Search covers the latest 100 snapshots. No live replay or hidden tool execution is exposed.

Client configuration (replace the repository path):

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
