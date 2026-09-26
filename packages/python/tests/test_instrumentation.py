import asyncio
import importlib
from types import SimpleNamespace

import pytest

import refract
from refract.instrumentation import Instrumentation


class SyncResource:
    def create(self, *, model, **kwargs):
        if kwargs.get("fail"):
            raise ValueError("private exception message")
        result = {
            "model": model,
            "output": [{"type": "function_call", "name": "lookup", "arguments": '{"id":1}'}],
            "usage": {
                "input_tokens": 100,
                "output_tokens": 20,
                "input_tokens_details": {"cached_tokens": 40},
            },
        }
        if kwargs.get("stream"):
            return iter(
                [
                    {"type": "response.output_text.delta", "delta": "Hello"},
                    {"type": "response.completed", "response": result},
                ]
            )
        return result


class AsyncResource:
    async def create(self, *, model, **kwargs):
        result = SyncResource().create(model=model, **kwargs)
        if kwargs.get("stream"):

            async def chunks():
                for chunk in result:
                    yield chunk

            return chunks()
        return result


def test_instrumented_provider_nested_spans_usage_tools_redaction_and_uninstall():
    original = SyncResource.create
    handle = Instrumentation(
        pricing={
            "demo": {"input_per_million": 2, "output_per_million": 5, "cache_read_per_million": 1}
        }
    )
    handle.patch(SyncResource, "create", "openai")
    try:
        with refract.run("agent") as run, refract.span(type="decision", name="planner") as planner:
            result = SyncResource().create(model="demo", input={"secret": "private", "text": "hi"})
            planner.output("done")
        events = run.data["events"]
        assert result["usage"]["input_tokens"] == 100
        assert events[1]["parent_id"] == events[0]["id"]
        assert events[1]["attributes"]["input_tokens"] == 100
        assert events[1]["attributes"]["cache_read_tokens"] == 40
        assert events[1]["attributes"]["cost_usd"] == pytest.approx(0.00026)
        assert events[1]["input"]["input"]["secret"] == "[REDACTED]"
        assert events[2]["name"] == "lookup"
        assert events[2]["parent_id"] == events[1]["id"]
        assert events[2]["attributes"]["tool_call_kind"] == "proposal"
        assert events[2]["replay_policy"] == "REQUIRES_APPROVAL"
        assert events[0]["status"] == "completed"
    finally:
        handle.uninstrument()
    assert SyncResource.create is original


def test_streaming_usage_ttft_and_automatic_standalone_run():
    handle = Instrumentation()
    handle.patch(SyncResource, "create", "openai")
    try:
        chunks = list(SyncResource().create(model="demo", input="hi", stream=True))
        assert len(chunks) == 2
        generation = handle.completed_runs[-1]["events"][0]
        assert generation["attributes"]["ttft_ms"] >= 0
        assert generation["attributes"]["output_tokens"] == 20
        assert generation["status"] == "completed"
        assert generation["output"]["model"] == "demo"
    finally:
        handle.uninstrument()


def test_async_stream_isolation_and_error_preservation():
    handle = Instrumentation()
    handle.patch(AsyncResource, "create", "openai")

    async def worker(name):
        with refract.run(name) as run:
            stream = await AsyncResource().create(model="demo", stream=True)
            assert len([chunk async for chunk in stream]) == 2
        return run.data

    async def main():
        return await asyncio.gather(worker("a"), worker("b"))

    try:
        left, right = asyncio.run(main())
        assert left["id"] != right["id"]
        assert all(e["run_id"] == left["id"] for e in left["events"])
        with pytest.raises(ValueError, match="private exception message"):
            asyncio.run(AsyncResource().create(model="demo", fail=True))
        assert (
            handle.completed_runs[-1]["events"][0]["attributes"]["exception_type"] == "ValueError"
        )
        assert "private exception message" not in str(handle.completed_runs[-1])
    finally:
        handle.uninstrument()


def test_anthropic_stream_partial_usage_and_early_close():
    class Resource:
        def create(self, **kwargs):
            return iter(
                [
                    {
                        "type": "message_start",
                        "message": {"usage": {"input_tokens": 12, "cache_read_input_tokens": 4}},
                    },
                    {
                        "type": "content_block_delta",
                        "delta": {"type": "text_delta", "text": "Hello"},
                    },
                    {"type": "message_delta", "usage": {"output_tokens": 8}},
                ]
            )

    handle = Instrumentation()
    handle.patch(Resource, "create", "anthropic")
    try:
        list(Resource().create(model="demo", stream=True))
        attrs = handle.completed_runs[-1]["events"][0]["attributes"]
        assert attrs["input_tokens"] == 12 and attrs["output_tokens"] == 8
        assert attrs["cache_read_tokens"] == 4
        with Resource().create(model="demo", stream=True) as stream:
            next(stream)
        event = handle.completed_runs[-1]["events"][0]
        assert event["attributes"]["stream_incomplete"] is True
    finally:
        handle.uninstrument()


def test_public_instrumentation_install_is_optional_and_idempotent(monkeypatch):
    original = SyncResource.create
    resource = SimpleNamespace(
        Responses=SyncResource,
        AsyncResponses=AsyncResource,
        Completions=SyncResource,
        AsyncCompletions=AsyncResource,
    )
    monkeypatch.setattr(importlib, "import_module", lambda name: resource)
    handle = refract.instrument_openai()
    another = refract.instrument_openai()
    try:
        SyncResource().create(model="demo")
        assert len(handle.completed_runs) == 1
        assert len(another.completed_runs) == 0
        another.uninstrument()
        assert SyncResource.create is not original
    finally:
        handle.uninstrument()
    assert SyncResource.create is original


def test_observation_failure_does_not_change_provider_result():
    class Unserializable:
        def model_dump(self, **kwargs):
            raise TypeError("cannot serialize")

    class Resource:
        def create(self, **kwargs):
            return "application result"

    handle = Instrumentation()
    handle.patch(Resource, "create", "custom")
    try:
        assert Resource().create(input=Unserializable()) == "application result"
        assert list(handle.errors) == ["TypeError"]
    finally:
        handle.uninstrument()
