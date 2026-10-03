"""One local smoke through PostgreSQL, Docker, SDKs, models, MCP, telemetry and SCIM.

Run against tests/integration/compose.yml. All credentials and model responses are fixtures.
"""

import asyncio
import copy
import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import time
import uuid
from pathlib import Path

from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client

import refract
from refract import RefractClient
from refract.artifact import pack, unpack
from refract.exporter import BackgroundExporter
from refract.otel import to_otlp

ROOT = Path(__file__).resolve().parents[2]
endpoint = os.environ.get("REFRACT_SERVER_URL", "http://127.0.0.1:51098")
key = os.environ.get("REFRACT_API_KEY", "local-smoke-admin-key-32-characters")
client = RefractClient(endpoint, api_key=key)
suffix = uuid.uuid4().hex[:8]
result_path = Path(os.environ.get("REFRACT_SMOKE_RESULT", "/tmp/refract-stack-results.json"))


def wait_for(check, seconds=60):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = check()
        if value:
            return value
        time.sleep(0.5)
    raise AssertionError("condition did not complete before deadline")


if "--verify-restart" in sys.argv:
    saved = json.loads(result_path.read_text())
    assert client.request("/v1/runs/" + saved["baseline"])["id"] == saved["baseline"]
    assert saved["baseline"] in [r["id"] for r in client.search_text("return policy")["runs"]]
    assert client.telemetry(kind="logs")["records"]
    assert client.request("/scim/v2/Users/" + saved["user"])["active"] is False
    print("Restart smoke passed: encrypted PostgreSQL runs, rebuilt vectors, telemetry and SCIM")
    raise SystemExit(0)

assert client.request("/v1/ready")["status"] == "ready"
client.configure_embeddings([{"profile": "local-demo", "is_default": True, "auto_index": True}])
fixture = json.loads((ROOT / "tests/fixtures/simple-run/execution.json").read_text())
fixture["id"] = "smoke-baseline-" + suffix
fixture["name"] = "Returns assistant · baseline"
fixture["metadata"]["api_key"] = "synthetic-secret-must-redact"
for event in fixture["events"]:
    event["run_id"] = fixture["id"]
fixture["events"][1]["attributes"].update(
    {
        "provider": "local-demo",
        "model": "local-demo-model",
        "input_tokens": 42,
        "output_tokens": 14,
        "total_tokens": 56,
        "cost_usd": 0.00084,
        "ttft_ms": 82,
    }
)
# These are deliberately measured-fixture values, not a provider's reported bill.
baseline = client.request("/v1/runs", fixture)
assert baseline["metadata"]["api_key"] == "[REDACTED]"
candidate = copy.deepcopy(fixture)
candidate["id"] = "smoke-candidate-" + suffix
candidate["name"] = "Returns assistant · candidate"
for event in candidate["events"]:
    event["run_id"] = candidate["id"]
candidate["events"][1]["output"]["text"] = "You can return your order within 14 days."
candidate = client.request("/v1/runs", candidate)
wait_for(lambda: client.embedding_settings()["jobs"]["pending"] == 0)
assert baseline["id"] in [
    r["id"] for r in client.search_text("return policy", mode="exact")["runs"]
]
assert baseline["id"] in [
    r["id"] for r in client.search_text("return policy", mode="approximate")["runs"]
]
branch = client.rerun(baseline["id"], profile="local-demo", from_event="evt_2", allow_live=True)
assert branch["id"] != baseline["id"] and len(branch["events"]) == 2
report = client.compare(baseline["id"], candidate["id"], grader="local-demo", allow_live=True)
assert not report["semantic_report"]["passed"]
assert "return window" in json.dumps(report).lower(), report
client.request("/v1/traces", to_otlp(baseline))

with tempfile.TemporaryDirectory(prefix="refract-spool-") as directory:
    exporter = BackgroundExporter(endpoint, api_key=key, spool_dir=directory)
    with refract.run("Python durable delivery", exporter=exporter, fail_open=False):
        refract.event(type="tool.call", name="Local lookup", output={"found": True})
    assert exporter.flush(timeout=15)
    exporter.close()

subprocess.run(
    [sys.executable, ROOT / "examples/otel/ingest.py"],
    check=True,
    env={**os.environ, "REFRACT_SERVER_URL": endpoint, "REFRACT_API_KEY": key},
)
assert "redacted-fixture" not in json.dumps(client.telemetry(kind="logs"))
assert (
    client.telemetry(kind="metrics")["records"][0]["payload"]["metric"]["sum"]["dataPoints"][0][
        "asInt"
    ]
    == 1
)
user = client.request(
    "/scim/v2/Users", {"userName": "demo-" + suffix, "externalId": "demo-" + suffix}
)
client.request(
    "/scim/v2/Users/" + user["id"],
    {
        "schemas": ["urn:ietf:params:scim:api:messages:2.0:PatchOp"],
        "Operations": [{"op": "replace", "path": "active", "value": False}],
    },
    method="PATCH",
)
assert client.request("/scim/v2/Users/" + user["id"])["active"] is False


async def mcp():
    params = StdioServerParameters(
        command=sys.executable,
        args=["-m", "refract_mcp"],
        env={
            **os.environ,
            "REFRACT_SERVER_URL": endpoint,
            "REFRACT_API_KEY": key,
            "REFRACT_MCP_ALLOW_LIVE": "1",
            "REFRACT_MCP_ALLOW_WRITES": "1",
        },
    )
    async with stdio_client(params) as (read, write), ClientSession(read, write) as session:
        await session.initialize()
        tools = {tool.name for tool in (await session.list_tools()).tools}
        assert {"telemetry_records", "rerun_models", "grade_runs", "search_text"} <= tools
        for name, args in [
            ("inspect_run", {"run_id": baseline["id"]}),
            ("search_text", {"query": "return policy"}),
            ("telemetry_records", {"kind": "logs"}),
        ]:
            result = await session.call_tool(name, args)
            assert not result.isError, result


asyncio.run(mcp())
with tempfile.TemporaryDirectory(prefix="refract-artifacts-") as directory:
    directory = Path(directory)
    first, second = directory / "baseline.rfr", directory / "candidate.rfr"
    first.write_bytes(pack(baseline))
    second.write_bytes(pack(candidate))
    assert unpack(first.read_bytes()) == baseline
    subprocess.run(["node", ROOT / "tests/contract/artifact.mjs", first], check=True)
    cli = ROOT / "target/debug/refract"
    if not cli.exists() or os.environ.get("REFRACT_SMOKE_DOCKER_CLI") == "1":
        assert os.environ.get("REFRACT_TEST_IMAGE"), "Set REFRACT_TEST_IMAGE for the Docker CLI"
        cli = directory / "refract"
        cli.write_text(
            '#!/bin/sh\nexec docker run --rm --network none --user "$(id -u):$(id -g)" '
            '--mount "type=bind,source=$REFRACT_SMOKE_ARTIFACTS,'
            'target=$REFRACT_SMOKE_ARTIFACTS,readonly" '
            '--entrypoint refract "$REFRACT_TEST_IMAGE" "$@"\n'
        )
        cli.chmod(0o700)
        os.environ["REFRACT_SMOKE_ARTIFACTS"] = str(directory)
    module_spec = importlib.util.spec_from_file_location(
        "regression", ROOT / "packages/github-action/compare.py"
    )
    module = importlib.util.module_from_spec(module_spec)
    module_spec.loader.exec_module(module)
    try:
        assert module.compare(first, first, str(cli))["passed"]
        assert not module.compare(first, second, str(cli))["passed"]
    except subprocess.CalledProcessError as error:
        print(error.stderr.decode() if isinstance(error.stderr, bytes) else error.stderr)
        raise

subprocess.run(
    ["node", ROOT / "tests/integration/full_stack.mjs"],
    check=True,
    env={**os.environ, "REFRACT_SERVER_URL": endpoint, "REFRACT_API_KEY": key},
)
wait_for(lambda: client.request("/v1/admin/outbox")["pending"] == 0)
assert client.request("/v1/admin/audit")
result_path.write_text(
    json.dumps(
        {
            "baseline": baseline["id"],
            "candidate": candidate["id"],
            "branch": branch["id"],
            "user": user["id"],
        }
    )
)
print(
    "Combined stack passed: Python/Node, CLI artifacts/action, MCP, PostgreSQL, embeddings, rerun/grading, OTLP, SCIM and drained delivery outbox"
)
