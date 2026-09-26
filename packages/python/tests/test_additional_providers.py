"""Real SDK contracts using intercepted HTTP only; no credentials or provider charges."""

import asyncio
import json
import struct
import zlib

import pytest

import refract


@pytest.mark.parametrize("vertex", [False, True])
@pytest.mark.parametrize("asynchronous", [False, True])
@pytest.mark.parametrize("streaming", [False, True])
def test_google_and_vertex_real_sdk(vertex, asynchronous, streaming):
    genai = pytest.importorskip("google.genai")
    import httpx

    response = {
        "candidates": [
            {"content": {"role": "model", "parts": [{"text": "Hello"}]}, "finishReason": "STOP"}
        ],
        "usageMetadata": {
            "promptTokenCount": 12,
            "candidatesTokenCount": 4,
            "cachedContentTokenCount": 3,
            "thoughtsTokenCount": 2,
        },
        "modelVersion": "fixture-model",
    }

    def send(request):
        assert request.url.host == "fixture.invalid"
        assert json.loads(request.content)["contents"][0]["parts"][0]["text"] == "Hi"
        if streaming:
            return httpx.Response(
                200,
                headers={"content-type": "text/event-stream"},
                content=f"data: {json.dumps(response)}\n\n",
            )
        return httpx.Response(200, json=response)

    transport = httpx.MockTransport(send)
    client = genai.Client(
        vertexai=vertex,
        api_key="fixture-secret",
        http_options={
            "base_url": "https://fixture.invalid",
            "httpx_client": httpx.Client(transport=transport),
            "httpx_async_client": httpx.AsyncClient(transport=transport),
        },
    )
    handle = refract.instrument_google(client)

    async def call_async():
        if streaming:
            stream = await client.aio.models.generate_content_stream(
                model="fixture-model", contents="Hi"
            )
            assert [chunk.text async for chunk in stream] == ["Hello"]
        else:
            assert (
                await client.aio.models.generate_content(model="fixture-model", contents="Hi")
            ).text == "Hello"
        await client.aio.aclose()

    try:
        with refract.run("google-contract") as run:
            if asynchronous:
                asyncio.run(call_async())
            elif streaming:
                assert [
                    chunk.text
                    for chunk in client.models.generate_content_stream(
                        model="fixture-model", contents="Hi"
                    )
                ] == ["Hello"]
            else:
                assert (
                    client.models.generate_content(model="fixture-model", contents="Hi").text
                    == "Hello"
                )
        event = run.data["events"][0]
        assert event["status"] == "completed"
        assert event["attributes"]["provider"] == ("vertex" if vertex else "google")
        assert event["attributes"]["input_tokens"] == 12
        assert event["attributes"]["output_tokens"] == 6
        assert event["attributes"]["cache_read_tokens"] == 3
        if streaming:
            assert event["attributes"]["ttft_ms"] >= 0
        assert "fixture-secret" not in json.dumps(run.snapshot())
        assert not handle.errors
    finally:
        handle.uninstrument()
        client.close()


@pytest.mark.parametrize("asynchronous", [False, True])
@pytest.mark.parametrize("streaming", [False, True])
def test_azure_real_sdk(asynchronous, streaming):
    sdk = pytest.importorskip("openai")
    import httpx

    response = {
        "id": "r1",
        "object": "response",
        "created_at": 1,
        "status": "completed",
        "model": "fixture-model",
        "output": [],
        "usage": {"input_tokens": 12, "output_tokens": 4, "total_tokens": 16},
    }

    def send(request):
        assert request.url.host == "fixture.invalid"
        if streaming:
            chunks = [
                {"type": "response.output_text.delta", "delta": "Hello"},
                {"type": "response.completed", "response": response},
            ]
            return httpx.Response(
                200,
                headers={"content-type": "text/event-stream"},
                content="".join(f"data: {json.dumps(chunk)}\n\n" for chunk in chunks),
            )
        return httpx.Response(200, json=response)

    transport = httpx.MockTransport(send)
    handle = refract.instrument_azure()

    async def call_async():
        async with sdk.AsyncAzureOpenAI(
            api_key="fixture-secret",
            api_version="2024-10-21",
            azure_endpoint="https://fixture.invalid",
            http_client=httpx.AsyncClient(transport=transport),
        ) as client:
            result = await client.responses.create(
                model="fixture-model", input="Hi", stream=streaming
            )
            if streaming:
                assert [chunk async for chunk in result]

    try:
        if asynchronous:
            asyncio.run(call_async())
        else:
            with sdk.AzureOpenAI(
                api_key="fixture-secret",
                api_version="2024-10-21",
                azure_endpoint="https://fixture.invalid",
                http_client=httpx.Client(transport=transport),
            ) as client:
                result = client.responses.create(
                    model="fixture-model", input="Hi", stream=streaming
                )
                if streaming:
                    assert list(result)
        event = handle.completed_runs[-1]["events"][0]
        assert event["attributes"]["provider"] == "azure"
        assert event["attributes"]["input_tokens"] == 12
        assert event["attributes"]["output_tokens"] == 4
        assert not handle.errors
    finally:
        handle.uninstrument()


def event_frame(event_type, payload):
    """Encode AWS event stream frames so botocore exercises its real stream parser."""
    headers = b""
    for key, value in {
        ":message-type": "event",
        ":event-type": event_type,
        ":content-type": "application/json",
    }.items():
        name, value = key.encode(), value.encode()
        headers += bytes([len(name)]) + name + b"\x07" + struct.pack(">H", len(value)) + value
    data = json.dumps(payload).encode()
    prelude = struct.pack(">II", 16 + len(headers) + len(data), len(headers))
    frame = prelude + struct.pack(">I", zlib.crc32(prelude)) + headers + data
    return frame + struct.pack(">I", zlib.crc32(frame))


@pytest.mark.parametrize("streaming", [False, True])
def test_bedrock_real_sdk_and_binary_event_stream(monkeypatch, streaming):
    boto3 = pytest.importorskip("boto3")
    from botocore.awsrequest import AWSResponse

    client = boto3.client(
        "bedrock-runtime",
        region_name="us-east-1",
        aws_access_key_id="fixture",
        aws_secret_access_key="fixture-secret",
        endpoint_url="https://fixture.invalid",
    )
    usage = {"inputTokens": 12, "outputTokens": 4, "totalTokens": 16}
    output = {
        "message": {
            "role": "assistant",
            "content": [
                {"text": "Hello"},
                {"toolUse": {"toolUseId": "call-1", "name": "lookup", "input": {"id": 1}}},
            ],
        }
    }
    if streaming:
        body = b"".join(
            [
                event_frame(
                    "contentBlockDelta", {"contentBlockIndex": 0, "delta": {"text": "Hello"}}
                ),
                event_frame(
                    "contentBlockStart",
                    {
                        "contentBlockIndex": 1,
                        "start": {"toolUse": {"toolUseId": "call-1", "name": "lookup"}},
                    },
                ),
                event_frame(
                    "contentBlockDelta",
                    {"contentBlockIndex": 1, "delta": {"toolUse": {"input": '{"id":1}'}}},
                ),
                event_frame("metadata", {"usage": usage, "metrics": {"latencyMs": 1}}),
            ]
        )
    else:
        body = json.dumps(
            {
                "output": output,
                "usage": usage,
                "stopReason": "tool_use",
                "metrics": {"latencyMs": 1},
            }
        ).encode()

    class Raw:
        def stream(self, *args, **kwargs):
            yield body

        def close(self):
            pass

    def send(request):
        assert request.url.startswith("https://fixture.invalid")
        return AWSResponse(
            request.url,
            200,
            {
                "content-type": "application/vnd.amazon.eventstream"
                if streaming
                else "application/json"
            },
            Raw(),
        )

    monkeypatch.setattr(client._endpoint.http_session, "send", send)
    handle = refract.instrument_bedrock(client)
    try:
        method = client.converse_stream if streaming else client.converse
        result = method(
            modelId="fixture-model", messages=[{"role": "user", "content": [{"text": "Hi"}]}]
        )
        if streaming:
            assert len(list(result["stream"])) == 4
        else:
            assert result["output"]["message"]["content"][0]["text"] == "Hello"
        run = handle.completed_runs[-1]
        event, tool = run["events"]
        assert event["attributes"]["provider"] == "bedrock"
        assert event["attributes"]["model"] == "fixture-model"
        assert event["attributes"]["input_tokens"] == 12
        assert event["attributes"]["output_tokens"] == 4
        assert tool["input"] == {"id": 1}
        assert tool["replay_policy"] == "REQUIRES_APPROVAL"
        assert "fixture-secret" not in json.dumps(run)
        assert not handle.errors
    finally:
        handle.uninstrument()
        client.close()


@pytest.mark.parametrize("asynchronous", [False, True])
def test_custom_client_normalization_and_response_identity(asynchronous):
    class Output:
        text = "Hello"

    result = Output()

    class Client:
        def ask(self, prompt, **kwargs):
            return result

        async def ask_async(self, prompt, **kwargs):
            return result

    client = Client()
    method = "ask_async" if asynchronous else "ask"
    original = getattr(client, method)
    handle = refract.instrument_custom(
        client,
        method,
        provider="private",
        model="local-weights",
        request=lambda args, kwargs: {"input": args[0]},
        response=lambda value: {
            "text": value.text,
            "usage": {"input_tokens": 2, "output_tokens": 1},
        },
    )
    try:
        output = getattr(client, method)("Hi", api_key="do-not-record")
        if asynchronous:
            output = asyncio.run(output)
        assert output is result
        event = handle.completed_runs[-1]["events"][0]
        assert event["input"] == {"input": "Hi", "model": "local-weights"}
        assert event["output"]["text"] == "Hello"
        assert event["attributes"]["total_tokens"] == 3
        assert "do-not-record" not in json.dumps(event)
    finally:
        handle.uninstrument()
    assert getattr(client, method) == original


def test_custom_async_generator_early_close_and_fail_open():
    class Client:
        async def stream(self, prompt):
            yield {"delta": "Hello"}
            yield {"delta": "there"}

    client = Client()
    handle = refract.instrument_custom(
        client,
        "stream",
        provider="private",
        model="local",
        request=lambda args, kwargs: {"input": args[0]},
        streaming=True,
    )

    async def call():
        stream = client.stream("Hi")
        assert await anext(stream) == {"delta": "Hello"}
        await stream.aclose()

    try:
        asyncio.run(call())
        event = handle.completed_runs[-1]["events"][0]
        assert event["attributes"]["stream_incomplete"] is True
        assert event["attributes"]["ttft_ms"] >= 0
    finally:
        handle.uninstrument()


def test_custom_response_normalizer_failure_preserves_original_result():
    class Client:
        def ask(self):
            return "application result"

    def invalid(value):
        raise TypeError("private detail")

    client = Client()
    handle = refract.instrument_custom(client, "ask", provider="private", response=invalid)
    try:
        assert client.ask() == "application result"
        assert list(handle.errors) == ["TypeError"]
    finally:
        handle.uninstrument()
