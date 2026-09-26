import base64
import json
import urllib.request

import pytest

import refract
from refract.integrations.langfuse import export_langfuse, to_langfuse
from refract.otel import from_otlp


def recording():
    with refract.run(
        "agent", metadata={"session_id": "session-1", "environment": "test", "tags": ["canary"]}
    ) as run:
        with refract.span(type="decision", name="request", input="Hi") as root:
            refract.event(
                type="generation",
                name="answer",
                input={"prompt": "Hi", "api_key": "not-exported"},
                output="Hello",
                attributes={
                    "provider": "custom",
                    "model": "local",
                    "input_tokens": 2,
                    "output_tokens": 1,
                    "total_tokens": 3,
                    "cost_usd": 0.001,
                },
            )
            root.output("Hello")
    return run.snapshot()


def test_langfuse_observation_mapping_and_context_propagation():
    run = recording()
    document = to_langfuse(run)
    root, child = document["resourceSpans"][0]["scopeSpans"][0]["spans"]
    assert root["traceId"] == child["traceId"]
    assert child["parentSpanId"] == root["spanId"]
    attrs = {item["key"]: item["value"] for item in child["attributes"]}
    assert attrs["langfuse.trace.name"] == {"stringValue": "agent"}
    assert attrs["langfuse.session.id"] == {"stringValue": "session-1"}
    assert attrs["langfuse.environment"] == {"stringValue": "test"}
    assert attrs["langfuse.observation.type"] == {"stringValue": "generation"}
    assert json.loads(attrs["langfuse.observation.usage_details"]["stringValue"]) == {
        "input": 2,
        "output": 1,
        "total": 3,
    }
    assert json.loads(attrs["langfuse.observation.cost_details"]["stringValue"]) == {
        "total": 0.001
    }
    assert "not-exported" not in json.dumps(document)
    assert from_otlp(document)[0]["events"][1]["output"] == "Hello"
    assert document == to_langfuse(run)


@pytest.mark.parametrize("rejected", [False, True])
def test_langfuse_http_credentials_are_headers_only_and_partial_rejection_fails(monkeypatch, rejected):
    captured = []

    class Response:
        def __enter__(self):
            return self

        def __exit__(self, *args):
            pass

        def read(self):
            return b'{"partialSuccess":{"rejectedSpans":"1"}}' if rejected else b"{}"

    def send(request, *, timeout):
        captured.append(request)
        assert timeout == 3
        return Response()

    monkeypatch.setattr(urllib.request, "urlopen", send)
    kwargs = dict(public_key="pk-test", secret_key="sk-private", timeout=3)
    if rejected:
        with pytest.raises(RuntimeError, match="partial rejection"):
            export_langfuse(recording(), "https://fixture.invalid", **kwargs)
    else:
        export_langfuse(recording(), "https://fixture.invalid", **kwargs)
    request = captured[0]
    headers = {key.lower(): value for key, value in request.headers.items()}
    assert request.full_url == "https://fixture.invalid/api/public/otel/v1/traces"
    assert headers["x-langfuse-ingestion-version"] == "4"
    assert headers["authorization"] == "Basic " + base64.b64encode(b"pk-test:sk-private").decode()
    assert b"sk-private" not in request.data


def test_langfuse_rejects_running_observations_before_network():
    with refract.run("running") as run:
        refract.event(type="decision", name="started")
        with pytest.raises(ValueError, match="completed recording"):
            to_langfuse(run.snapshot())


def test_langfuse_rejects_empty_recording():
    with refract.run("empty") as run:
        pass
    with pytest.raises(ValueError, match="at least one event"):
        to_langfuse(run.snapshot())
