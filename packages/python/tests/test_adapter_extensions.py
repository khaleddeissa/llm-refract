"""Offline contracts: no external endpoints, credentials or model downloads."""

import asyncio
import io
import json
from types import SimpleNamespace
from uuid import uuid4

import pytest

import refract


@pytest.mark.parametrize("asynchronous", [False, True])
@pytest.mark.parametrize("streaming", [False, True])
def test_native_bedrock_preserves_lazy_response(asynchronous, streaming):
    payload = {"generation": "hello", "prompt_token_count": 12, "generation_token_count": 4}
    encoded = json.dumps(payload).encode()
    body = io.BytesIO(encoded)
    if streaming:
        body = iter([{"chunk": {"bytes": encoded}}])
    response = {"body": body, "ResponseMetadata": {"RequestId": "fixture"}}

    def invoke(**kwargs):
        assert kwargs["modelId"] == "test-model"
        if asynchronous:

            async def resolve():
                return response

            return resolve()
        return response

    name = "invoke_model_with_response_stream" if streaming else "invoke_model"
    client = SimpleNamespace(**{name: invoke})
    handle = refract.instrument_bedrock_native(client)
    with refract.run("native") as run:
        result = getattr(client, name)(modelId="test-model", body='{"prompt":"hello"}')
        if asynchronous:
            result = asyncio.run(result)
        assert result is response
        assert run.data["events"][0]["status"] == "running"
        if streaming:
            assert list(result["body"]) == [{"chunk": {"bytes": encoded}}]
        else:
            assert body.tell() == 0
            assert result["body"].read(0) == b""
            assert run.data["events"][0]["status"] == "running"
            assert b"".join(result["body"].iter_chunks(8)) == encoded
    attrs = run.data["events"][0]["attributes"]
    assert (attrs["input_tokens"], attrs["output_tokens"]) == (12, 4)
    assert not handle.errors
    handle.uninstrument()
    assert getattr(client, name) is invoke


def test_body_failure_and_early_close_are_observed():
    class Broken:
        def read(self, amt=None):
            raise OSError("fixture")

        def close(self):
            pass

    client = SimpleNamespace(invoke_model=lambda **kwargs: {"body": Broken()})
    handle = refract.instrument_bedrock_native(client)
    with pytest.raises(OSError):
        client.invoke_model(body="{}", modelId="test")["body"].read()
    assert handle.completed_runs[-1]["events"][0]["status"] == "failed"
    client.invoke_model(body="{}", modelId="test")["body"].close()
    assert handle.completed_runs[-1]["events"][0]["attributes"]["stream_incomplete"]


@pytest.mark.parametrize(
    "library,method",
    [
        ("ollama", "chat"),
        ("huggingface", "chat_completion"),
        ("llama_cpp", "create_chat_completion"),
        ("litellm", "acompletion"),
    ],
)
def test_library_adapters_and_awaitable_methods(library, method):
    value = {"message": {"content": "hello"}, "prompt_eval_count": 3, "eval_count": 2}

    async def resolve():
        return value

    client = SimpleNamespace(**{method: lambda **kwargs: resolve()})
    handle = refract.instrument_library(client, library)
    assert asyncio.run(getattr(client, method)(model="local", messages=[])) is value
    assert len(handle.completed_runs) == 1
    assert handle.completed_runs[0]["events"][0]["attributes"]["provider"] == library
    if library == "ollama":
        assert handle.completed_runs[0]["events"][0]["attributes"]["total_tokens"] == 5
    assert not handle.errors


def test_langchain_provider_capture_is_one_generation():
    pytest.importorskip("langchain_core")
    from refract.integrations.langchain import langchain_handler

    client = SimpleNamespace(
        create=lambda **kwargs: {"usage": {"input_tokens": 3, "output_tokens": 2}}
    )
    handle = refract.instrument_custom(client, "create", provider="fixture")
    with refract.run("combined") as run:
        handler = langchain_handler()
        identity = uuid4()
        handler.on_llm_start({}, ["hello"], run_id=identity)
        client.create(model="local", messages=[])
        handler.on_llm_end({"llm_output": {}}, run_id=identity)
        client.create(model="local", messages=[])
    assert len(run.data["events"]) == 2
    assert run.data["events"][0]["attributes"]["provider_instrumented"]
    assert run.data["events"][0]["attributes"]["total_tokens"] == 5
    assert not handle.errors and not handler.errors


@pytest.mark.parametrize("asynchronous", [False, True])
def test_realtime_response_lifecycle_and_audio_exclusion(asynchronous):
    events = [
        {"type": "response.created", "response": {"id": "r1"}},
        {"type": "response.output_audio.delta", "response_id": "r1", "delta": "secret-audio"},
        {"type": "response.output_text.delta", "response_id": "r1", "delta": "hello"},
        {
            "type": "response.done",
            "response": {
                "id": "r1",
                "status": "completed",
                "output": [{"audio": "secret-audio", "text": "hello"}],
                "usage": {"input_tokens": 3, "output_tokens": 2},
            },
        },
    ]
    iterator = iter(events)

    def recv():
        value = next(iterator)
        if asynchronous:

            async def resolve():
                return value

            return resolve()
        return value

    connection = SimpleNamespace(recv=recv, close=lambda: None)
    handle = refract.instrument_realtime(connection)
    with refract.run("realtime") as run:
        for expected in events:
            actual = connection.recv()
            if asynchronous:
                actual = asyncio.run(actual)
            assert actual is expected
        connection.close()
    assert len(run.data["events"]) == 1
    event = run.data["events"][0]
    assert event["attributes"]["total_tokens"] == 5
    assert "secret-audio" not in json.dumps(run.snapshot())
    assert not handle.errors


@pytest.mark.parametrize("bearer", [False, True])
@pytest.mark.parametrize("asynchronous", [False, True])
def test_foundry_v1_endpoint_and_rotating_auth(bearer, asynchronous):
    sdk = pytest.importorskip("openai")
    import httpx

    seen = []

    def send(request):
        assert request.url.host == "foundry.invalid"
        assert request.url.path == "/openai/v1/chat/completions"
        assert json.loads(request.content)["model"] == "deployment-name"
        seen.append(request.headers["authorization"])
        return httpx.Response(
            200,
            json={
                "id": "c1",
                "object": "chat.completion",
                "created": 1,
                "model": "deployment-name",
                "choices": [
                    {
                        "index": 0,
                        "finish_reason": "stop",
                        "message": {"role": "assistant", "content": "hello"},
                    }
                ],
                "usage": {"prompt_tokens": 3, "completion_tokens": 2, "total_tokens": 5},
            },
        )

    tokens = iter(["fixture-one", "fixture-two"])
    key = (lambda: next(tokens)) if bearer else "fixture-key"
    if bearer and asynchronous:

        async def async_key():
            return next(tokens)

        key = async_key
    transport = httpx.MockTransport(send)

    def make_client(async_client):
        cls, http = (
            (sdk.AsyncOpenAI, httpx.AsyncClient) if async_client else (sdk.OpenAI, httpx.Client)
        )
        return cls(
            base_url="https://foundry.invalid/openai/v1/",
            api_key=key,
            http_client=http(transport=transport),
        )

    client = make_client(asynchronous)
    handle = refract.instrument_azure(client)

    async def call():
        for _ in range(2):
            await client.chat.completions.create(model="deployment-name", messages=[])
        await client.close()

    try:
        if asynchronous:
            asyncio.run(call())
        else:
            for _ in range(2):
                client.chat.completions.create(model="deployment-name", messages=[])
            client.close()
        assert seen == (
            ["Bearer fixture-one", "Bearer fixture-two"] if bearer else ["Bearer fixture-key"] * 2
        )
        assert len(handle.completed_runs) == 2
        assert "fixture-key" not in json.dumps(list(handle.completed_runs))
        assert not handle.errors
    finally:
        handle.uninstrument()


@pytest.mark.parametrize("asynchronous", [False, True])
def test_actual_langchain_invocation_deduplicates_provider_generation(asynchronous):
    pytest.importorskip("langchain_core")
    from langchain_core.language_models.chat_models import BaseChatModel
    from langchain_core.messages import AIMessage
    from langchain_core.outputs import ChatGeneration, ChatResult

    from refract.integrations.langchain import langchain_handler

    client = SimpleNamespace(
        create=lambda **kwargs: {"usage": {"input_tokens": 3, "output_tokens": 2}}
    )
    handle = refract.instrument_custom(client, "create", provider="fixture")

    class LocalChat(BaseChatModel):
        @property
        def _llm_type(self):
            return "local-fixture"

        def _generate(self, messages, stop=None, run_manager=None, **kwargs):
            client.create(model="fixture", messages=[])
            return ChatResult(
                generations=[
                    ChatGeneration(
                        message=AIMessage(
                            content="hello",
                            usage_metadata={
                                "input_tokens": 3,
                                "output_tokens": 2,
                                "total_tokens": 5,
                            },
                        )
                    )
                ]
            )

        async def _agenerate(self, messages, stop=None, run_manager=None, **kwargs):
            return self._generate(messages, stop, run_manager, **kwargs)

    try:
        with refract.run("framework-contract") as recording:
            handler = langchain_handler()
            model = LocalChat()
            if asynchronous:
                asyncio.run(model.ainvoke("hello", config={"callbacks": [handler]}))
            else:
                model.invoke("hello", config={"callbacks": [handler]})
        generations = [e for e in recording.data["events"] if e["type"] == "generation"]
        assert len(generations) == 1
        assert generations[0]["attributes"]["total_tokens"] == 5
        assert not handle.errors and not handler.errors
    finally:
        handle.uninstrument()


def test_native_bedrock_real_botocore_streaming_body():
    boto3 = pytest.importorskip("boto3")
    from botocore.response import StreamingBody
    from botocore.stub import Stubber

    client = boto3.client(
        "bedrock-runtime",
        region_name="us-east-1",
        aws_access_key_id="fixture",
        aws_secret_access_key="fixture",
    )
    body = json.dumps(
        {
            "content": [{"type": "text", "text": "hello"}],
            "usage": {"input_tokens": 3, "output_tokens": 2},
        }
    ).encode()
    with Stubber(client) as stub:
        stub.add_response(
            "invoke_model",
            {"body": StreamingBody(io.BytesIO(body), len(body)), "contentType": "application/json"},
        )
        handle = refract.instrument_bedrock_native(client)
        response = client.invoke_model(modelId="fixture", body='{"messages":[]}')
        assert response["body"].read() == body
        assert handle.completed_runs[0]["events"][0]["attributes"]["total_tokens"] == 5
        assert not handle.errors
        handle.uninstrument()


def test_realtime_uninstrument_finishes_pending_and_ignores_unidentified_responses():
    from types import SimpleNamespace

    values = iter(
        [
            {"type": "response.created", "response": {}},
            {"type": "response.created", "response": {"id": "partial"}},
        ]
    )
    connection = SimpleNamespace(recv=lambda: next(values))
    handle = refract.instrument_realtime(connection)
    with refract.run("cleanup") as run:
        connection.recv()
        connection.recv()
        handle.uninstrument()
    assert len(run.snapshot()["events"]) == 1
    assert run.snapshot()["events"][0]["attributes"]["stream_incomplete"] is True
