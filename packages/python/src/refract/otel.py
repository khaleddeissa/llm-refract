"""OpenTelemetry OTLP/HTTP JSON bridge; no OTel runtime dependency is required."""

from __future__ import annotations

import hashlib
import json
import re
import urllib.request
from datetime import UTC, datetime
from typing import Any

from . import SPEC_VERSION, _snapshot


def _id(value: str, length: int) -> str:
    return hashlib.sha256(value.encode()).hexdigest()[:length]


def _ns(value: str) -> int:
    return int(datetime.fromisoformat(value.replace("Z", "+00:00")).timestamp() * 1_000_000_000)


def _time(value: int) -> str:
    return datetime.fromtimestamp(value / 1_000_000_000, UTC).isoformat()


def _value(value: Any) -> dict:
    if isinstance(value, bool):
        return {"boolValue": value}
    if isinstance(value, int):
        return {"intValue": str(value)}
    if isinstance(value, float):
        return {"doubleValue": value}
    if isinstance(value, str):
        return {"stringValue": value}
    if isinstance(value, (list, tuple)):
        return {"arrayValue": {"values": [_value(item) for item in value]}}
    if isinstance(value, dict):
        return {"kvlistValue": {"values": _attributes(value)}}
    return {}


def _attributes(values: dict) -> list[dict]:
    return [{"key": key, "value": _value(value)} for key, value in values.items()]


def _decode(value: dict) -> Any:
    for key, cast in (
        ("stringValue", str),
        ("boolValue", bool),
        ("intValue", int),
        ("doubleValue", float),
    ):
        if key in value:
            return cast(value[key])
    if "arrayValue" in value:
        return [_decode(item) for item in value["arrayValue"].get("values", [])]
    if "kvlistValue" in value:
        return _decode_attributes(value["kvlistValue"].get("values", []))
    return None


def _decode_attributes(values: list) -> dict:
    return {item["key"]: _decode(item["value"]) for item in values}


def to_otlp(recording: dict, *, service_name: str = "llm-refract") -> dict:
    """Encode one run as OTLP JSON traces, preserving event payloads in namespaced attributes."""
    recording = _snapshot(recording)
    trace_id = _id(recording["id"], 32)
    spans = []
    for event in recording["events"]:
        start = _ns(event["timestamp"])
        attributes = {
            "refract.event.id": event["id"],
            "refract.event.type": event["type"],
            "refract.event.input": json.dumps(event.get("input"), ensure_ascii=False),
            "refract.event.output": json.dumps(event.get("output"), ensure_ascii=False),
            "refract.event.attributes": json.dumps(event.get("attributes", {}), ensure_ascii=False),
            "refract.event.replay_policy": event["replay_policy"],
            "refract.event.status": event["status"],
        }
        for source, target in {
            "model": "gen_ai.request.model",
            "provider": "gen_ai.provider.name",
            "input_tokens": "gen_ai.usage.input_tokens",
            "output_tokens": "gen_ai.usage.output_tokens",
        }.items():
            if source in event["attributes"]:
                attributes[target] = event["attributes"][source]
        span = {
            "traceId": trace_id,
            "spanId": _id(event["id"], 16),
            "name": event["name"],
            "kind": 3 if event["type"] == "generation" else 1,
            "startTimeUnixNano": str(start),
            "endTimeUnixNano": str(start + int(event["duration_ms"] * 1_000_000)),
            "attributes": _attributes(attributes),
            "status": {"code": 2 if event["status"] == "failed" else 1},
        }
        if event.get("parent_id"):
            span["parentSpanId"] = _id(event["parent_id"], 16)
        spans.append(span)
    resource = {
        "service.name": service_name,
        "refract.run.id": recording["id"],
        "refract.run.name": recording["name"],
        "refract.run.metadata": json.dumps(recording.get("metadata", {})),
        "refract.run.started_at": recording["started_at"],
        "refract.run.ended_at": recording.get("ended_at"),
        "refract.run.status": recording["status"],
    }
    return {
        "resourceSpans": [
            {
                "resource": {"attributes": _attributes(resource)},
                "scopeSpans": [{"scope": {"name": "llm-refract"}, "spans": spans}],
            }
        ]
    }


def from_otlp(document: dict) -> list[dict]:
    """Import one canonical run per trace, sorting parents before children.

    External parents remain in otel.external_parent_id. Unsupported OTel span events
    and links are retained under attributes; no application code is executed.
    """
    groups: dict[str, list] = {}
    for resource_spans in document.get("resourceSpans", []):
        resource = _decode_attributes(resource_spans.get("resource", {}).get("attributes", []))
        for scope in resource_spans.get("scopeSpans", []):
            for span in scope.get("spans", []):
                trace_id, span_id = span.get("traceId", ""), span.get("spanId", "")
                if not re.fullmatch(r"[0-9a-fA-F]{32}", trace_id) or int(trace_id, 16) == 0:
                    raise ValueError("invalid OTLP trace ID")
                if not re.fullmatch(r"[0-9a-fA-F]{16}", span_id) or int(span_id, 16) == 0:
                    raise ValueError("invalid OTLP span ID")
                item = dict(span, spanId=span_id.lower())
                if item.get("parentSpanId"):
                    item["parentSpanId"] = item["parentSpanId"].lower()
                groups.setdefault(trace_id.lower(), []).append((item, resource))
    runs = []
    for trace_id, entries in groups.items():
        resource = entries[0][1]
        run_id = resource.get("refract.run.id", f"run_otel_{trace_id}")
        starts = [int(item[0]["startTimeUnixNano"]) for item in entries]
        ends = [int(item[0]["endTimeUnixNano"]) for item in entries]
        run = {
            "spec_version": SPEC_VERSION,
            "id": run_id,
            "name": resource.get("refract.run.name", resource.get("service.name", "OTel trace")),
            "started_at": resource.get("refract.run.started_at", _time(min(starts))),
            "ended_at": resource.get("refract.run.ended_at", _time(max(ends))),
            "status": resource.get("refract.run.status", "completed"),
            "metadata": json.loads(resource.get("refract.run.metadata", "{}")),
            "events": [],
        }
        mapped = {}
        for span, _ in entries:
            if span["spanId"] in mapped:
                raise ValueError("duplicate OTLP span ID within trace")
            attrs = _decode_attributes(span.get("attributes", []))
            mapped[span["spanId"]] = attrs.get("refract.event.id", f"evt_{span['spanId']}")
        if len(set(mapped.values())) != len(mapped):
            raise ValueError("duplicate mapped Refract event IDs")
        pending = list(entries)
        emitted: set[str] = set()
        while pending:
            previous_length = len(pending)
            for span, span_resource in list(pending):
                parent = span.get("parentSpanId")
                if parent in mapped and parent not in emitted:
                    continue
                attributes = _decode_attributes(span.get("attributes", []))
                values = json.loads(attributes.get("refract.event.attributes", "{}"))
                for source, target in {
                    "gen_ai.request.model": "model",
                    "gen_ai.provider.name": "provider",
                    "gen_ai.system": "provider",
                    "gen_ai.usage.input_tokens": "input_tokens",
                    "gen_ai.usage.output_tokens": "output_tokens",
                }.items():
                    if source in attributes:
                        values[target] = attributes[source]
                if "refract.event.id" not in attributes:
                    values["otel.attributes"] = attributes
                    values["otel.resource"] = span_resource
                if parent and parent not in mapped:
                    values["otel.external_parent_id"] = parent
                for field in ("events", "links"):
                    if field in span:
                        values[f"otel.{field}"] = span[field]
                start, end = int(span["startTimeUnixNano"]), int(span["endTimeUnixNano"])
                if end < start:
                    raise ValueError("OTLP span ends before it starts")
                failed = span.get("status", {}).get("code") in (2, "STATUS_CODE_ERROR")
                if failed:
                    run["status"] = "failed"
                run["events"].append(
                    {
                        "id": mapped[span["spanId"]],
                        "run_id": run_id,
                        "parent_id": mapped.get(parent),
                        "type": attributes.get(
                            "refract.event.type", "generation" if "model" in values else "decision"
                        ),
                        "name": span["name"],
                        "timestamp": _time(start),
                        "duration_ms": (end - start) / 1_000_000,
                        "status": attributes.get(
                            "refract.event.status", "failed" if failed else "completed"
                        ),
                        "input": json.loads(attributes.get("refract.event.input", "null")),
                        "output": json.loads(attributes.get("refract.event.output", "null")),
                        "attributes": values,
                        "replay_policy": attributes.get("refract.event.replay_policy", "RECORDED"),
                    }
                )
                emitted.add(span["spanId"])
                pending.remove((span, span_resource))
            if len(pending) == previous_length:
                raise ValueError("OTLP spans contain a parent cycle")
        runs.append(_snapshot(run))
    return runs


def from_spans(spans) -> list[dict]:
    """Convert completed OpenTelemetry SDK ReadableSpans without an OTLP dependency.

    Supply a complete trace where possible. This function only imports the supplied
    spans: it does not reconstruct missing parents or join different export batches.
    """
    resource_spans = []
    for source in spans:
        context = source.get_span_context()
        if context is None or source.start_time is None or source.end_time is None:
            raise ValueError("OTel SDK spans must have context and completed timestamps")
        item = {
            "traceId": format(context.trace_id, "032x"),
            "spanId": format(context.span_id, "016x"),
            "name": source.name,
            "startTimeUnixNano": str(source.start_time),
            "endTimeUnixNano": str(source.end_time),
            "attributes": _attributes(dict(source.attributes or {})),
            "status": {"code": source.status.status_code.value},
            "events": [
                {
                    "name": event.name,
                    "timeUnixNano": str(event.timestamp),
                    "attributes": _attributes(dict(event.attributes or {})),
                }
                for event in source.events
            ],
            "links": [
                {
                    "traceId": format(link.context.trace_id, "032x"),
                    "spanId": format(link.context.span_id, "016x"),
                    "attributes": _attributes(dict(link.attributes or {})),
                }
                for link in source.links
            ],
        }
        if source.parent:
            item["parentSpanId"] = format(source.parent.span_id, "016x")
        resource_spans.append(
            {
                "resource": {"attributes": _attributes(dict(source.resource.attributes))},
                "scopeSpans": [{"spans": [item]}],
            }
        )
    return from_otlp({"resourceSpans": resource_spans})


def export_otlp(recording: dict, endpoint: str, *, headers: dict | None = None, timeout=10) -> None:
    """Synchronously POST OTLP JSON to an OTel collector's /v1/traces endpoint."""
    _send_otlp(to_otlp(recording), endpoint, headers=headers, timeout=timeout)


def _send_otlp(document: dict, endpoint: str, *, headers: dict | None = None, timeout=10) -> None:
    address = endpoint.rstrip("/")
    if not address.endswith("/v1/traces"):
        address += "/v1/traces"
    request = urllib.request.Request(
        address,
        json.dumps(document, allow_nan=False).encode(),
        {"Content-Type": "application/json", **(headers or {})},
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=timeout) as response:
        body = response.read()
    acknowledgement = json.loads(body) if body else {}
    partial = acknowledgement.get("partialSuccess") or {}
    if int(partial.get("rejectedSpans", 0)) or partial.get("errorMessage"):
        raise RuntimeError("OTLP collector reported partial rejection")
