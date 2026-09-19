# Refract MCP package

Executable stdio interface to the Rust Refract API: ten read-only tools, plus import/fork tools when
`REFRACT_MCP_ALLOW_WRITES=1`. Start with `uv run refract-mcp`.

See [configuration, tool inventory and limits](../../docs/usage/mcp.md).
Implementation: [server.py](src/refract_mcp/server.py).
