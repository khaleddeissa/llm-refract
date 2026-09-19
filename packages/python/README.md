# llm-refract

Provider-neutral Python instrumentation for AI applications, plus an optional MCP interface to the
Refract execution engine. Export portable `.rfr` files or submit snapshots to the Refract REST service.

## Install

```bash
pip install llm-refract
```

MCP support is an optional extra:

```bash
pip install "llm-refract[mcp]"
```

## SDK usage

```python
import refract

with refract.run("agent", path="run.rfr"):
    refract.event(type="generation", name="answer", output={"text": "Hello"})
```

See the [usage guide](https://github.com/khaleddeissa/llm-refract/blob/main/docs/usage/python.md).

## MCP server

Executable stdio interface to the Rust Refract API: ten read-only tools, plus import/fork tools when
`REFRACT_MCP_ALLOW_WRITES=1`. Requires the `mcp` extra above. Start with:

```bash
refract-mcp
```

See [configuration, tool inventory and limits](https://github.com/khaleddeissa/llm-refract/blob/main/docs/usage/mcp.md).
Implementation: [server.py](src/refract_mcp/server.py).
