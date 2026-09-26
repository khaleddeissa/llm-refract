"""Exercise batched SDK delivery, indexed search, semantic evaluation and MCP metrics."""

import asyncio
import json
import os
import sys
import urllib.request
import uuid

from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client

import refract
from refract.exporter import BackgroundExporter

endpoint = os.environ.get("REFRACT_SERVER_URL", "http://127.0.0.1:8000").rstrip("/")
key = os.environ.get("REFRACT_API_KEY")
headers = {"Content-Type": "application/json"}
if key:
    headers["Authorization"] = "Bearer " + key


def api(path, body=None):
    request = urllib.request.Request(
        endpoint + path, None if body is None else json.dumps(body).encode(), headers
    )
    with urllib.request.urlopen(request, timeout=15) as response:
        return json.load(response)


name = "platform-" + uuid.uuid4().hex
exporter = BackgroundExporter(endpoint, api_key=key, batch_size=2)
for label in ("baseline", "candidate"):
    with refract.run(name + "-" + label, exporter=exporter, fail_open=False) as run:
        refract.event(
            type="generation",
            name="answer",
            output={"text": "Returns within 30 days."},
            attributes={
                "model": "local-fixture",
                "provider": "custom",
                "input_tokens": 8,
                "output_tokens": 6,
                "total_tokens": 14,
                "cost_usd": 0.01,
            },
        )
    if label == "baseline":
        baseline = run.snapshot()
    else:
        candidate = run.snapshot()
assert exporter.flush(timeout=10), "background ingestion did not finish"
exporter.close()
page = api("/v1/search?q=" + name + "&model=local-fixture&limit=1")
assert page["total"] == 2 and len(page["runs"]) == 1
metrics = api("/v1/runs/" + baseline["id"] + "/metrics")
assert metrics["total_tokens"] == 14 and metrics["cost_usd"] == 0.01
result = api(
    "/v1/eval",
    {
        "pairs": [{"name": "same-output", "left": baseline["id"], "right": candidate["id"]}],
        "options": {},
    },
)
assert result["passed"], result
# An ambiguous HTTP response can cause a transport retry; identical snapshots stay idempotent.
receipt = api("/v1/runs/batch", {"runs": [baseline, candidate]})
assert receipt["accepted"] == 0, receipt


async def check_mcp():
    params = StdioServerParameters(
        command=sys.executable,
        args=["-m", "refract_mcp"],
        env={**os.environ, "REFRACT_SERVER_URL": endpoint},
    )
    async with stdio_client(params) as (read, write), ClientSession(read, write) as session:
        await session.initialize()
        result = await session.call_tool("run_metrics", {"run_id": baseline["id"]})
        assert not result.isError, result
        assert json.loads(result.content[0].text)["total_tokens"] == 14


asyncio.run(check_mcp())
print("Platform integration passed: batch, retry, search, usage, eval and MCP")
