"""Installed LiteLLM contracts using its mock responses; network and telemetry are disabled."""

import asyncio
import socket

import pytest

import refract


@pytest.fixture
def litellm_client(monkeypatch):
    monkeypatch.setenv("LITELLM_LOCAL_MODEL_COST_MAP", "True")
    monkeypatch.setenv("LITELLM_TELEMETRY", "False")

    def no_network(*args, **kwargs):
        raise AssertionError("LiteLLM contract attempted a network connection")

    monkeypatch.setattr(socket.socket, "connect", no_network)
    monkeypatch.setattr(socket.socket, "connect_ex", no_network)
    litellm = pytest.importorskip("litellm")
    monkeypatch.setattr(litellm, "telemetry", False)
    monkeypatch.setattr(litellm, "disable_hf_tokenizer_download", True)
    # Tokenizer assets are external to this contract; no model weights or token files are fetched.
    monkeypatch.setattr(litellm, "token_counter", lambda *args, **kwargs: 3)
    monkeypatch.setattr(litellm.utils, "token_counter", lambda *args, **kwargs: 3)
    return litellm


@pytest.mark.parametrize("asynchronous", [False, True])
@pytest.mark.parametrize("streaming", [False, True])
def test_real_litellm_chat_mock(litellm_client, asynchronous, streaming):
    handle = refract.instrument_litellm(litellm_client)
    args = {
        "model": "openai/gpt-4o-mini",
        "messages": [{"role": "user", "content": "hello"}],
        "mock_response": "local answer",
        "stream": streaming,
    }

    async def execute():
        response = await litellm_client.acompletion(**args)
        if streaming:
            return [chunk async for chunk in response]
        return response

    try:
        with refract.run("litellm actual SDK") as run:
            if asynchronous:
                result = asyncio.run(execute())
            else:
                result = litellm_client.completion(**args)
                if streaming:
                    result = list(result)
        assert result
        generations = [event for event in run.data["events"] if event["type"] == "generation"]
        assert len(generations) == 1
        assert generations[0]["status"] == "completed"
        if streaming:
            text = "".join(
                (choice.get("delta") or {}).get("content") or ""
                for chunk in generations[0]["output"]["chunks"]
                for choice in chunk.get("choices", [])
            )
            assert text == "local answer"
        else:
            assert "local answer" in str(generations[0]["output"])
        assert not handle.errors
    finally:
        handle.uninstrument()


@pytest.mark.parametrize("asynchronous", [False, True])
def test_real_litellm_responses_mock(litellm_client, asynchronous):
    handle = refract.instrument_litellm()
    try:
        with refract.run("litellm responses") as run:
            args = {
                "model": "openai/gpt-4o-mini",
                "input": "preserved input",
                "mock_response": "local answer",
            }
            if asynchronous:
                asyncio.run(litellm_client.aresponses(**args))
            else:
                litellm_client.responses(**args)
        generations = [event for event in run.data["events"] if event["type"] == "generation"]
        assert len(generations) == 1
        assert generations[0]["input"]["input"] == "preserved input"
        assert generations[0]["status"] == "completed"
        assert not handle.errors
    finally:
        handle.uninstrument()
