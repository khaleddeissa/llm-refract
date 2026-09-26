"""Read-oriented MCP facade over the Rust REST service; never executes tools."""

import json
import os
import urllib.parse
import urllib.request
from typing import Any

from mcp.server.fastmcp import FastMCP
from mcp.types import ToolAnnotations

mcp = FastMCP(
    "refract", instructions="Inspect captured executions. No live execution is supported."
)
READ = ToolAnnotations(readOnlyHint=True, destructiveHint=False, openWorldHint=False)


def api(path: str, body: dict | None = None) -> Any:
    endpoint = os.environ.get("REFRACT_SERVER_URL", "http://127.0.0.1:8000").rstrip("/")
    headers = {"Content-Type": "application/json"}
    if key := os.environ.get("REFRACT_API_KEY"):
        headers["Authorization"] = "Bearer " + key
    request = urllib.request.Request(
        endpoint + path,
        None if body is None else json.dumps(body).encode(),
        headers,
    )
    with urllib.request.urlopen(request, timeout=15) as response:
        data = response.read(17 * 1024 * 1024 + 1)
    if len(data) > 17 * 1024 * 1024:
        raise ValueError("response exceeds size limit")
    return json.loads(data)


def run_path(run_id: str) -> str:
    if not run_id or run_id in {".", ".."}:
        raise ValueError("invalid run id")
    return "/v1/runs/" + urllib.parse.quote(run_id, safe="")


@mcp.tool(annotations=READ)
def search_runs(
    query: str = "",
    status: str = "",
    model: str = "",
    tool: str = "",
    min_duration_ms: float | None = None,
    limit: int = 100,
    offset: int = 0,
) -> list[dict]:
    """Query stored executions using server-side filters and pagination."""
    if status not in {"", "running", "completed", "failed"}:
        raise ValueError("unsupported status")
    if not 1 <= limit <= 1000 or offset < 0:
        raise ValueError("invalid pagination")
    params: dict[str, Any] = {
        "q": query,
        "status": status,
        "model": model,
        "tool": tool,
        "limit": limit,
        "offset": offset,
    }
    if min_duration_ms is not None:
        if min_duration_ms < 0:
            raise ValueError("duration must be nonnegative")
        params["min_duration_ms"] = min_duration_ms
    return api("/v1/search?" + urllib.parse.urlencode(params))["runs"]


@mcp.tool(annotations=READ)
def list_failed_runs() -> list[dict]:
    """Return the first page of failed runs using an indexed server query."""
    return search_runs(status="failed")


@mcp.tool(annotations=READ)
def inspect_run(run_id: str) -> dict:
    """Read metadata and all recorded events for a run."""
    return api(run_path(run_id))


@mcp.tool(annotations=READ)
def inspect_event(run_id: str, event_id: str) -> dict:
    """Read one recorded event, including input, output and replay policy."""
    for event in inspect_run(run_id)["events"]:
        if event["id"] == event_id:
            return event
    raise ValueError("event not found")


@mcp.tool(annotations=READ)
def show_execution_graph(run_id: str) -> dict:
    """Return nodes and parent-child edges in recorded order."""
    events = inspect_run(run_id)["events"]
    return {
        "nodes": [{k: e[k] for k in ("id", "name", "type", "status")} for e in events],
        "edges": [{"from": e["parent_id"], "to": e["id"]} for e in events if e["parent_id"]],
    }


@mcp.tool(annotations=READ)
def compare_runs(left: str, right: str, semantic: bool = False, threshold: float = 0.75) -> dict:
    """Compare recorded event semantics; does not execute either run."""
    return api(
        "/v1/diff",
        {
            "left": left,
            "right": right,
            "semantic": semantic,
            "options": {"similarity_threshold": threshold},
        },
    )


@mcp.tool(annotations=READ)
def find_first_divergence(left: str, right: str) -> dict:
    """Return the first differing event position and its before/after values."""
    result = compare_runs(left, right)
    return {
        "index": result["first_divergence"],
        "difference": next(iter(result["differences"]), None),
    }


@mcp.tool(annotations=READ)
def export_run(run_id: str) -> dict:
    """Return the canonical snapshot and artifact download path, without writing files."""
    return {"execution": inspect_run(run_id), "artifact_path": run_path(run_id) + "/artifact"}


@mcp.resource("refract://capabilities")
def capabilities() -> str:
    """Describe this server's supported operations and limits."""
    return json.dumps(
        {
            "transport": "stdio",
            "read_only": os.environ.get("REFRACT_MCP_ALLOW_WRITES") != "1",
            "pagination": True,
            "metrics": True,
            "semantic_diff": True,
            "live_replay": False,
            "spec_version": "refract.execution.v1",
        }
    )


def main() -> None:
    mcp.run(transport="stdio")


@mcp.tool(annotations=READ)
def health() -> dict:
    """Check API readiness and describe the supported storage/replay mode."""
    return {"readiness": api("/v1/ready"), "replay": "recorded"}


@mcp.tool(annotations=READ)
def replay_recorded(run_id: str) -> dict:
    """Return captured outputs. Never executes tools/models; BLOCKED policies are enforced."""
    return api(run_path(run_id) + "/replay", {"mode": "exact"})


if os.environ.get("REFRACT_MCP_ALLOW_WRITES") == "1":
    WRITE = ToolAnnotations(readOnlyHint=False, destructiveHint=False, openWorldHint=False)

    @mcp.tool(annotations=WRITE)
    def fork_run(run_id: str, from_event: str) -> dict:
        """Persist a prefix branch before an event. Does not execute new steps."""
        return api(run_path(run_id) + "/fork", {"from_event": from_event})

    @mcp.tool(annotations=WRITE)
    def import_run(execution: dict) -> dict:
        """Store a canonical snapshot through Rust validation/redaction; duplicate IDs conflict."""
        return api("/v1/runs", execution)


@mcp.tool(annotations=READ)
def run_metrics(run_id: str) -> dict:
    """Read measured token, cost, latency, cache and failure totals; unknown prices stay null."""
    return api(run_path(run_id) + "/metrics")


@mcp.tool(annotations=READ)
def evaluate_runs(pairs: list[dict], options: dict | None = None) -> dict:
    """Evaluate named baseline/candidate ID pairs with semantic and metric budgets. No execution."""
    return api("/v1/eval", {"pairs": pairs, "options": options or {}})


@mcp.tool(annotations=READ)
def similar_runs(run_id: str, limit: int = 10) -> dict:
    """Find runs with similar recorded failure/event text using lexical ranking."""
    if not 1 <= limit <= 100:
        raise ValueError("limit must be between 1 and 100")
    return api(run_path(run_id) + "/similar?" + urllib.parse.urlencode({"limit": limit}))
