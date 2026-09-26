import copy

import pytest

import refract
from refract.otel import export_otlp, from_otlp, from_spans, to_otlp


def test_otlp_roundtrip_preserves_graph_payload_usage_and_run_identity():
    with refract.run("agent", metadata={"environment": "test"}) as run:
        with refract.span(type="decision", name="parent"):
            refract.event(
                type="generation",
                name="answer",
                input={"text": "hello"},
                output={"text": "world"},
                attributes={"input_tokens": 3, "model": "demo", "secret": "private"},
                duration_ms=1.2,
            )
    document = to_otlp(run.snapshot())
    spans = document["resourceSpans"][0]["scopeSpans"][0]["spans"]
    spans.reverse()
    imported = from_otlp(document)[0]
    assert imported["id"] == run.data["id"]
    assert imported["metadata"] == run.data["metadata"]
    assert imported["events"][1]["parent_id"] == imported["events"][0]["id"]
    assert imported["events"][1]["output"] == {"text": "world"}
    assert imported["events"][1]["attributes"]["input_tokens"] == 3
    assert imported["events"][1]["attributes"]["secret"] == "[REDACTED]"
    assert imported["events"][1]["duration_ms"] == pytest.approx(1.2)


def test_import_native_otlp_and_reject_parent_cycles():
    span = {
        "traceId": "1" * 32,
        "spanId": "2" * 16,
        "name": "llm",
        "startTimeUnixNano": "1700000000000000000",
        "endTimeUnixNano": "1700000000001000000",
        "status": {"code": 2},
        "attributes": [
            {"key": "gen_ai.request.model", "value": {"stringValue": "demo"}},
            {"key": "gen_ai.usage.input_tokens", "value": {"intValue": "7"}},
        ],
    }
    document = {"resourceSpans": [{"scopeSpans": [{"spans": [span]}]}]}
    result = from_otlp(document)[0]
    assert result["status"] == "failed"
    assert result["events"][0]["type"] == "generation"
    assert result["events"][0]["attributes"]["input_tokens"] == 7
    span["parentSpanId"] = span["spanId"]
    with pytest.raises(ValueError, match="cycle"):
        from_otlp(document)
    malformed = copy.deepcopy(document)
    malformed["resourceSpans"][0]["scopeSpans"][0]["spans"][0]["traceId"] = "wrong"
    with pytest.raises(ValueError, match="trace ID"):
        from_otlp(malformed)


def test_real_otel_sdk_import_preserves_nested_spans():
    trace = pytest.importorskip("opentelemetry.sdk.trace")
    from opentelemetry.sdk.trace.export import SimpleSpanProcessor
    from opentelemetry.sdk.trace.export.in_memory_span_exporter import InMemorySpanExporter

    provider = trace.TracerProvider()
    exporter = InMemorySpanExporter()
    provider.add_span_processor(SimpleSpanProcessor(exporter))
    tracer = provider.get_tracer("fixture")
    with tracer.start_as_current_span("agent"):
        with tracer.start_as_current_span(
            "answer",
            attributes={
                "gen_ai.request.model": "fixture-model",
                "gen_ai.usage.input_tokens": 8,
            },
        ) as span:
            span.add_event("first token", attributes={"phase": "stream"})
    recording = from_spans(exporter.get_finished_spans())[0]
    assert [event["name"] for event in recording["events"]] == ["agent", "answer"]
    assert recording["events"][1]["parent_id"] == recording["events"][0]["id"]
    assert recording["events"][1]["attributes"]["input_tokens"] == 8
    assert recording["events"][1]["attributes"]["otel.events"][0]["name"] == "first token"
    provider.shutdown()


def test_otlp_http_posts_standard_json_with_headers(monkeypatch):
    import urllib.request

    captured = []

    class Response:
        def __enter__(self):
            return self

        def __exit__(self, *args):
            pass

        def read(self):
            return b"{}"

    def send(request, **kwargs):
        captured.append(request)
        return Response()

    monkeypatch.setattr(urllib.request, "urlopen", send)
    with refract.run("otlp") as recording:
        refract.event(type="decision", name="answer", output="Hello")
    export_otlp(
        recording.snapshot(), "http://collector:4318", headers={"Authorization": "Bearer test"}
    )
    assert captured[0].full_url == "http://collector:4318/v1/traces"
    assert captured[0].headers["Authorization"] == "Bearer test"
    import json

    assert from_otlp(json.loads(captured[0].data))[0]["name"] == "otlp"
