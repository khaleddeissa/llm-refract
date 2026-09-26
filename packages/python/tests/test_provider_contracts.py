"""Real optional SDKs, deterministic HTTP transports, no provider network requests."""

import asyncio
import importlib
import json

import pytest

import refract


def provider_transport(provider, *, streaming=False, failure=False):
    # SDK major versions may migrate their transport dependency independently.
    base_client = importlib.import_module(f"{provider}._base_client")
    http = getattr(base_client, "httpx", None) or base_client.httpx2
    if provider == "openai":
        response = {
            "id": "resp_test",
            "object": "response",
            "created_at": 1700000000,
            "model": "fixture-model",
            "status": "completed",
            "output": [
                {
                    "type": "function_call",
                    "id": "call_test",
                    "call_id": "call_test",
                    "name": "lookup",
                    "arguments": '{"query":"policy"}',
                }
            ],
            "usage": {"input_tokens": 12, "output_tokens": 4, "total_tokens": 16},
        }
        events = [
            {
                "type": "response.output_text.delta",
                "delta": "Hello",
                "item_id": "msg_test",
                "output_index": 0,
                "content_index": 0,
                "sequence_number": 1,
            },
            {"type": "response.completed", "response": response, "sequence_number": 2},
        ]
    else:
        response = {
            "id": "msg_test",
            "type": "message",
            "role": "assistant",
            "model": "fixture-model",
            "content": [{"type": "text", "text": "Hello"}],
            "stop_reason": "end_turn",
            "stop_sequence": None,
            "usage": {"input_tokens": 12, "output_tokens": 4},
        }
        events = [
            {
                "type": "message_start",
                "message": dict(
                    response, content=[], usage={"input_tokens": 12, "output_tokens": 0}
                ),
            },
            {
                "type": "content_block_start",
                "index": 0,
                "content_block": {"type": "text", "text": ""},
            },
            {
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": "Hello"},
            },
            {"type": "content_block_stop", "index": 0},
            {
                "type": "message_delta",
                "delta": {"stop_reason": "end_turn", "stop_sequence": None},
                "usage": {"output_tokens": 4},
            },
            {"type": "message_stop"},
        ]

    def handle(request):
        assert request.url.host == "fixture.invalid"
        if failure:
            return http.Response(
                400,
                json={
                    "error": {"type": "invalid_request_error", "message": "private provider detail"}
                },
            )
        if streaming:
            content = "".join(f"event: {e['type']}\ndata: {json.dumps(e)}\n\n" for e in events)
            return http.Response(
                200, headers={"content-type": "text/event-stream"}, content=content
            )
        return http.Response(200, json=response)

    return http, http.MockTransport(handle)


@pytest.mark.parametrize("provider", ["openai", "anthropic"])
@pytest.mark.parametrize("asynchronous", [False, True])
@pytest.mark.parametrize("streaming", [False, True])
def test_installed_provider_create_contract(provider, asynchronous, streaming):
    sdk = pytest.importorskip(provider)
    http, transport = provider_transport(provider, streaming=streaming)
    handle = getattr(refract, f"instrument_{provider}")()
    kwargs = {"model": "fixture-model", "stream": streaming}
    kwargs.update(input="Hi") if provider == "openai" else kwargs.update(
        messages=[{"role": "user", "content": "Hi"}], max_tokens=64
    )

    async def call_async():
        cls = sdk.AsyncOpenAI if provider == "openai" else sdk.AsyncAnthropic
        async with cls(
            api_key="fixture-key",
            base_url="https://fixture.invalid",
            http_client=http.AsyncClient(transport=transport),
        ) as client:
            resource = client.responses if provider == "openai" else client.messages
            result = await resource.create(**kwargs)
            if streaming:
                assert [event async for event in result]
            else:
                assert result.model == "fixture-model"

    try:
        with refract.run("real-sdk") as recording, refract.span(type="decision", name="parent"):
            if asynchronous:
                asyncio.run(call_async())
            else:
                cls = sdk.OpenAI if provider == "openai" else sdk.Anthropic
                with cls(
                    api_key="fixture-key",
                    base_url="https://fixture.invalid",
                    http_client=http.Client(transport=transport),
                ) as client:
                    resource = client.responses if provider == "openai" else client.messages
                    result = resource.create(**kwargs)
                    if streaming:
                        assert list(result)
                    else:
                        assert result.model == "fixture-model"
        event = recording.data["events"][1]
        assert event["parent_id"] == recording.data["events"][0]["id"]
        assert event["status"] == "completed"
        assert event["attributes"]["input_tokens"] == 12
        assert event["attributes"]["output_tokens"] == 4
        if streaming:
            assert event["attributes"]["ttft_ms"] >= 0
        assert list(handle.errors) == []
    finally:
        handle.uninstrument()


@pytest.mark.parametrize("provider", ["openai", "anthropic"])
def test_installed_provider_errors_are_preserved_without_exception_body(provider):
    sdk = pytest.importorskip(provider)
    http, transport = provider_transport(provider, failure=True)
    handle = getattr(refract, f"instrument_{provider}")()
    cls = sdk.OpenAI if provider == "openai" else sdk.Anthropic
    try:
        with cls(
            api_key="fixture-key",
            base_url="https://fixture.invalid",
            max_retries=0,
            http_client=http.Client(transport=transport),
        ) as client:
            with pytest.raises(sdk.BadRequestError, match="private provider detail"):
                if provider == "openai":
                    client.responses.create(model="fixture-model", input="Hi")
                else:
                    client.messages.create(
                        model="fixture-model",
                        max_tokens=64,
                        messages=[{"role": "user", "content": "Hi"}],
                    )
        assert handle.completed_runs[-1]["status"] == "failed"
        assert "private provider detail" not in json.dumps(handle.completed_runs[-1])
        assert "fixture-key" not in json.dumps(handle.completed_runs[-1])
    finally:
        handle.uninstrument()
