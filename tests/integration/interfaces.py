"""Exercise Python SDK and MCP writes/read tools against the running Rust service."""

import asyncio
import json
import os
import sys
import urllib.request

from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client

import refract
from refract.artifact import unpack

endpoint = os.environ.get("REFRACT_SERVER_URL", "http://127.0.0.1:8000")
with refract.run("python-http-integration", endpoint=endpoint) as recording:
    refract.event(type="tool.call", name="lookup", output={"found": True})
with urllib.request.urlopen(
    endpoint + "/v1/runs/" + recording.data["id"] + "/artifact"
) as response:
    assert unpack(response.read())["events"][0]["output"]["found"]


async def check_mcp():
    params = StdioServerParameters(
        command=sys.executable,
        args=["-m", "refract_mcp"],
        env={**os.environ, "REFRACT_SERVER_URL": endpoint, "REFRACT_MCP_ALLOW_WRITES": "1"},
    )
    async with stdio_client(params) as (read, write), ClientSession(read, write) as session:
        await session.initialize()
        result = await session.call_tool("replay_recorded", {"run_id": recording.data["id"]})
        assert not result.isError
        forked = await session.call_tool(
            "fork_run",
            {"run_id": recording.data["id"], "from_event": recording.data["events"][0]["id"]},
        )
        assert not forked.isError
        branch = json.loads(forked.content[0].text)
        assert branch["events"] == []
        with refract.run("mcp-import") as imported:
            refract.event(type="decision", name="choose", output=1)
        result = await session.call_tool("import_run", {"execution": imported.snapshot()})
        assert not result.isError
        duplicate = await session.call_tool("import_run", {"execution": imported.snapshot()})
        assert duplicate.isError


asyncio.run(check_mcp())
print("Python SDK and MCP → API integration passed")
