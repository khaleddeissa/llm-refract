"""Export completed recordings through Langfuse's OTLP/HTTP JSON interface."""

from __future__ import annotations

import base64
import json

from refract import _snapshot
from refract.otel import _attributes, _send_otlp, to_otlp


def to_langfuse(recording: dict) -> dict:
    """Map a finished run to observations, retaining Refract's portable attributes.

    IDs are deterministic. Export each finished recording once: repeated exports
    can create duplicate observations in the receiving Langfuse deployment.
    """
    recording = _snapshot(recording)
    if not recording["events"]:
        raise ValueError("Langfuse export requires at least one event")
    if recording["status"] == "running" or any(
        event["status"] == "running" for event in recording["events"]
    ):
        raise ValueError("Langfuse export requires a completed recording")
    document = to_otlp(recording)
    shared = {"langfuse.trace.name": recording["name"]}
    metadata = recording.get("metadata", {})
    for source, destination in {
        "session_id": "langfuse.session.id",
        "user_id": "langfuse.user.id",
        "environment": "langfuse.environment",
        "release": "langfuse.release",
        "version": "langfuse.version",
        "tags": "langfuse.trace.tags",
    }.items():
        if source in metadata:
            shared[destination] = metadata[source]
    for key, value in metadata.items():
        shared[f"langfuse.trace.metadata.{key}"] = json.dumps(value, ensure_ascii=False)
    spans = document["resourceSpans"][0]["scopeSpans"][0]["spans"]
    for event, span in zip(recording["events"], spans, strict=True):
        attrs = event.get("attributes", {})
        mapped = {
            **shared,
            "langfuse.observation.type": {
                "generation": "generation",
                "tool.call": "tool",
                "retrieval": "retriever",
            }.get(event["type"], "span"),
            "langfuse.observation.input": json.dumps(event.get("input"), ensure_ascii=False),
            "langfuse.observation.output": json.dumps(event.get("output"), ensure_ascii=False),
            "langfuse.observation.level": "ERROR" if event["status"] == "failed" else "DEFAULT",
        }
        if "model" in attrs:
            mapped["langfuse.observation.model.name"] = attrs["model"]
        usage = {
            destination: attrs[source]
            for source, destination in {
                "input_tokens": "input",
                "output_tokens": "output",
                "total_tokens": "total",
            }.items()
            if source in attrs
        }
        if usage:
            mapped["langfuse.observation.usage_details"] = json.dumps(usage)
        if "cost_usd" in attrs:
            mapped["langfuse.observation.cost_details"] = json.dumps({"total": attrs["cost_usd"]})
        for key, value in attrs.items():
            mapped[f"langfuse.observation.metadata.{key}"] = json.dumps(value, ensure_ascii=False)
        span["attributes"].extend(_attributes(mapped))
    return document


def export_langfuse(
    recording: dict,
    base_url: str,
    *,
    public_key: str,
    secret_key: str,
    timeout: float = 10,
) -> None:
    """Explicit synchronous export to a Cloud region or self-hosted Langfuse base URL.

    No environment variables or global OTel configuration are modified. Credentials
    stay in the HTTP Authorization header and never enter the recording.
    """
    if not public_key or not secret_key:
        raise ValueError("Langfuse project public and secret keys are required")
    authorization = base64.b64encode(f"{public_key}:{secret_key}".encode()).decode("ascii")
    _send_otlp(
        to_langfuse(recording),
        base_url.rstrip("/") + "/api/public/otel/v1/traces",
        headers={"Authorization": f"Basic {authorization}", "x-langfuse-ingestion-version": "4"},
        timeout=timeout,
    )
