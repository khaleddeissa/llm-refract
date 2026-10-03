"""Read-oriented facade with separately enabled provider calls; never executes application tools."""

import json
import os
import urllib.parse
from typing import Any

from mcp.server.fastmcp import FastMCP
from mcp.types import ToolAnnotations

from refract.client import RefractClient

mcp = FastMCP(
    "refract",
    instructions="Inspect executions. Provider calls require the optional live tools and explicit allow_live consent.",
)
READ = ToolAnnotations(readOnlyHint=True, destructiveHint=False, openWorldHint=False)


def api(path: str, body: dict | None = None) -> Any:
    client = RefractClient(
        os.environ.get("REFRACT_SERVER_URL", "http://127.0.0.1:8000"),
        api_key=os.environ.get("REFRACT_API_KEY"),
    )
    return client.request(path, body, timeout=130 if body and body.get("allow_live") else 35)


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
            "live_replay": os.environ.get("REFRACT_MCP_ALLOW_LIVE") == "1"
            and os.environ.get("REFRACT_MCP_ALLOW_WRITES") == "1",
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


@mcp.tool(annotations=READ)
def vector_search(model: str, values: list[float], limit: int = 20) -> list[dict]:
    """Find recorded runs using an application-supplied embedding in the active tenant scope."""
    import math

    if not model.strip() or len(model) > 256 or not 1 <= len(values) <= 4096:
        raise ValueError("invalid embedding model or dimensions")
    if (
        not 1 <= limit <= 100
        or not any(values)
        or any(not math.isfinite(v) or abs(v) > 1e10 for v in values)
    ):
        raise ValueError("invalid embedding values or result limit")
    return api(
        "/v1/search/vector", {"embedding": {"model": model, "values": values}, "limit": limit}
    )


@mcp.tool(annotations=READ)
def embedding_models() -> dict:
    """List enabled project embedding profiles, available models, and index job counts."""
    return {**api("/v1/embedding-models"), **api("/v1/project/embeddings")}


@mcp.tool(annotations=ToolAnnotations(readOnlyHint=True, destructiveHint=False, openWorldHint=True))
def search_text(query: str, profile: str | None = None, limit: int = 20) -> dict:
    """Search executions by meaning; sends query text to the project's configured embedding provider."""
    if not query.strip() or len(query.encode()) > 8000 or not 1 <= limit <= 100:
        raise ValueError("query must be 1..8000 bytes and limit 1..100")
    return api("/v1/search/text", {"query": query, "profile": profile, "limit": limit})


@mcp.tool(annotations=READ)
def generation_models() -> list[dict]:
    """List operator-approved model profiles and domain grading availability for this project."""
    return api("/v1/generation-models")["models"]


if os.environ.get("REFRACT_MCP_ALLOW_LIVE") == "1":
    LIVE_READ = ToolAnnotations(
        readOnlyHint=True, destructiveHint=False, openWorldHint=True, idempotentHint=False
    )

    @mcp.tool(annotations=LIVE_READ)
    def grade_runs(
        left: str, right: str, grader: str, allow_live: bool = False, threshold: float = 0.75
    ) -> dict:
        """Call a configured model grader; requires explicit consent and can incur provider usage."""
        if not allow_live:
            raise ValueError("model grading requires allow_live=True")
        return api(
            "/v1/diff",
            {
                "left": left,
                "right": right,
                "semantic": True,
                "grader": grader,
                "allow_live": True,
                "options": {"similarity_threshold": threshold},
            },
        )

    if os.environ.get("REFRACT_MCP_ALLOW_WRITES") == "1":
        LIVE_WRITE = ToolAnnotations(
            readOnlyHint=False, destructiveHint=False, openWorldHint=True, idempotentHint=False
        )

        @mcp.tool(annotations=LIVE_WRITE)
        def rerun_models(
            run_id: str,
            from_event: str,
            profile: str,
            allow_live: bool = False,
            reuse_recorded: list[str] | None = None,
            approved_events: list[str] | None = None,
        ) -> dict:
            """Execute model steps into a new branch; explicitly reuse named other steps without running tools. Provider usage may be charged."""
            if not allow_live:
                raise ValueError("model rerun requires allow_live=True")
            return api(
                run_path(run_id) + "/rerun",
                {
                    "profile": profile,
                    "from_event": from_event,
                    "allow_live": True,
                    "reuse_recorded": reuse_recorded or [],
                    "approved_events": approved_events or [],
                },
            )


@mcp.tool(annotations=READ)
def telemetry_records(
    kind: str = "logs", trace_id: str = "", limit: int = 50, offset: int = 0
) -> dict:
    """Read scoped OTLP logs or metrics, optionally filtering logs by propagated trace ID."""
    if kind not in {"logs", "metrics"} or not 1 <= limit <= 100 or offset < 0:
        raise ValueError("invalid telemetry query")
    return api(
        "/v1/telemetry?"
        + urllib.parse.urlencode(
            {"kind": kind, "trace_id": trace_id, "limit": limit, "offset": offset}
        )
    )
