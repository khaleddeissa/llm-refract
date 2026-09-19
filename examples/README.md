# Examples

Run from the repository root after `make setup` and `npm run build -w @refract-ai/sdk`.
All recordings use deterministic demo data and require no provider credentials.
Outputs use exclusive creation; choose new filenames if rerunning.

| Example          | Command                                           | Demonstrates                              |
| ---------------- | ------------------------------------------------- | ----------------------------------------- |
| Python basic     | `uv run python examples/python/basic/record.py`   | Parent-child recording                    |
| Retrieval        | `uv run python examples/python/rag/record.py`     | Documents, generation, citations          |
| State            | `uv run python examples/python/state/record.py`   | Before/after state, blocked checkpoint    |
| Failure          | `uv run python examples/python/failure/record.py` | Error capture and redaction               |
| TypeScript basic | `node examples/typescript/basic/record.mjs`       | Local artifact writing                    |
| Concurrent runs  | `node examples/typescript/concurrent/record.mjs`  | Async context isolation                   |
| MCP              | `uv run python examples/mcp/client.py`            | Stdio initialization, tools and resources |

Send a completed Python run to the server by adding `endpoint="http://localhost:8000"`
to `refract.run`. TypeScript accepts the same endpoint in its options object.
