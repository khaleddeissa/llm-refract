import asyncio
import json
import sys
from unittest.mock import patch

from mcp.client.stdio import stdio_client
from refract_mcp import server

from mcp import ClientSession, StdioServerParameters


def test_read_tools():
    run = {
        "id": "x",
        "name": "demo",
        "status": "failed",
        "events": [
            {
                "id": "e",
                "name": "lookup",
                "type": "tool.call",
                "status": "failed",
                "parent_id": None,
            }
        ],
    }
    with patch.object(server, "api", return_value=[run]):
        assert server.list_failed_runs() == [run]
        assert server.search_runs("missing") == []
    with patch.object(server, "api", return_value=run):
        assert server.inspect_event("x", "e")["name"] == "lookup"
        assert server.show_execution_graph("x")["edges"] == []
        assert server.export_run("x")["artifact_path"] == "/v1/runs/x/artifact"
    assert server.run_path("a/b") == "/v1/runs/a%2Fb"


def test_real_stdio_handshake_and_discovery():
    async def exercise():
        params = StdioServerParameters(command=sys.executable, args=["-m", "refract_mcp"])
        async with stdio_client(params) as (read, write), ClientSession(read, write) as session:
            await session.initialize()
            tools = await session.list_tools()
            assert len(tools.tools) == 8
            assert all(t.annotations.readOnlyHint for t in tools.tools)
            resource = await session.read_resource("refract://capabilities")
            assert json.loads(resource.contents[0].text)["live_replay"] is False

    asyncio.run(asyncio.wait_for(exercise(), timeout=20))
