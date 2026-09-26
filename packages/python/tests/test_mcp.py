import asyncio
import json
import os
import sys
from unittest.mock import patch

import pytest
from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client

from refract_mcp import server


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
    with patch.object(server, "api", return_value={"runs": [run]}) as request:
        assert server.list_failed_runs() == [run]
        assert server.search_runs("missing") == [run]
        assert "q=missing" in request.call_args.args[0]
    with patch.object(server, "api", return_value=run):
        assert server.inspect_event("x", "e")["name"] == "lookup"
        assert server.show_execution_graph("x")["edges"] == []
        assert server.export_run("x")["artifact_path"] == "/v1/runs/x/artifact"
    assert server.run_path("a/b") == "/v1/runs/a%2Fb"


@pytest.mark.parametrize("writes", [False, True])
def test_real_stdio_handshake_and_discovery(writes):
    async def exercise():
        params = StdioServerParameters(
            command=sys.executable,
            args=["-m", "refract_mcp"],
            env={**os.environ, "REFRACT_MCP_ALLOW_WRITES": "1" if writes else "0"},
        )
        async with stdio_client(params) as (read, write), ClientSession(read, write) as session:
            await session.initialize()
            tools = await session.list_tools()
            assert len(tools.tools) == (15 if writes else 13)
            names = {t.name for t in tools.tools}
            assert ("fork_run" in names) == writes
            assert ("import_run" in names) == writes
            assert "replay_recorded" in names
            assert all(
                t.annotations.readOnlyHint
                for t in tools.tools
                if t.name not in {"fork_run", "import_run"}
            )
            resource = await session.read_resource("refract://capabilities")
            assert json.loads(resource.contents[0].text)["live_replay"] is False

    asyncio.run(asyncio.wait_for(exercise(), timeout=20))


def test_metrics_and_comparison_forward_options():
    with patch.object(server, "api", return_value={}) as request:
        server.compare_runs("a", "b", semantic=True, threshold=0.9)
        assert request.call_args.args[1]["options"]["similarity_threshold"] == 0.9
        server.run_metrics("a/b")
        assert request.call_args.args[0] == "/v1/runs/a%2Fb/metrics"
    with pytest.raises(ValueError):
        server.search_runs(limit=1001)
