<div align="center">

<img src="https://raw.githubusercontent.com/khaleddeissa/llm-refract/main/assets/llm-refract-logo.svg" width="96" height="96" alt="Refract green diamond" />

# llm-refract

**Provider-neutral Python instrumentation for AI systems, with an optional MCP interface.**

[![PyPI](https://img.shields.io/pypi/v/llm-refract?style=for-the-badge&label=PyPI&color=3775A9)](https://pypi.org/project/llm-refract/)
[![Python](https://img.shields.io/pypi/pyversions/llm-refract?style=for-the-badge&logo=python&logoColor=white)](https://pypi.org/project/llm-refract/)
[![Downloads](https://img.shields.io/pypi/dm/llm-refract?style=for-the-badge&color=2EA44F)](https://pypi.org/project/llm-refract/)
[![MCP](https://img.shields.io/badge/Model%20Context%20Protocol-MCP-000000?style=for-the-badge)](#mcp-server)
[![License](https://img.shields.io/badge/License-Apache--2.0-0D75B8?style=for-the-badge)](https://github.com/khaleddeissa/llm-refract/blob/main/LICENSE)

[Docs](https://github.com/khaleddeissa/llm-refract/tree/main/docs) · [Examples](https://github.com/khaleddeissa/llm-refract/tree/main/examples/python) · [Source](https://github.com/khaleddeissa/llm-refract/tree/main/packages/python) · [Issues](https://github.com/khaleddeissa/llm-refract/issues)

</div>

---

Instrument synchronous and asynchronous AI applications with provider-neutral events. Export portable
`.rfr` files or submit snapshots to the Refract REST service. Record model calls, tools, retrieval,
decisions, state changes, checkpoints and failures — then inspect, replay, fork and diff them.

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

Try `python examples/python/basic/record.py` after installing. Outputs go to `.examples/`.

See the [Python usage guide](https://github.com/khaleddeissa/llm-refract/blob/main/docs/usage/python.md)
for async patterns, redaction, failure capture and submitting to a remote endpoint.

## MCP server

Executable stdio interface to the Rust Refract API: thirteen read-only tools, plus import/fork tools when
`REFRACT_MCP_ALLOW_WRITES=1`. Requires the `mcp` extra above. Start with:

```bash
refract-mcp
```

See [configuration, tool inventory and limits](https://github.com/khaleddeissa/llm-refract/blob/main/docs/usage/mcp.md).
Implementation: [`server.py`](https://github.com/khaleddeissa/llm-refract/blob/main/packages/python/src/refract_mcp/server.py).

## Links

- [Documentation](https://github.com/khaleddeissa/llm-refract/tree/main/docs)
- [TypeScript / npm SDK](https://www.npmjs.com/package/@llm-refract/sdk)
- [Rust CLI and engine](https://github.com/khaleddeissa/llm-refract/tree/main/crates)
- [Issues](https://github.com/khaleddeissa/llm-refract/issues)

## License

Apache-2.0 — see [LICENSE](https://github.com/khaleddeissa/llm-refract/blob/main/LICENSE).

See [provider coverage](../../docs/usage/providers.md) for Azure, Gemini/Vertex, Bedrock and custom
model adapters, and [production operation](../../docs/production.md) for reliable remote capture.
