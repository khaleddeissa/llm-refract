"""No network: exercise trusted provider dispatch, normalization and replay preflight."""

import asyncio
import copy
from types import SimpleNamespace as NS

import pytest

import refract
from refract.rerun import ExecutorRegistry, rerun


def fixture(input=None):
    with refract.run("provider replay") as run:
        refract.event(type="retrieval", name="search", input={}, output="context")
        refract.event(
            type="generation",
            name="answer",
            input=input or {"messages": [{"role": "user", "content": "hello"}], "model": "old"},
            output="old",
            attributes={
                "model": "old",
                "total_tokens": 999,
                "cost_usd": 4,
                "stream_incomplete": True,
            },
        )
    return run.snapshot()


@pytest.mark.parametrize(
    "provider,api,input",
    [
        ("openai", "chat", {"messages": [{"role": "user", "content": "hello"}]}),
        ("azure", "responses", {"input": "hello", "max_output_tokens": 40}),
        (
            "anthropic",
            "auto",
            {"messages": [{"role": "user", "content": "hello"}], "max_tokens": 40},
        ),
        ("google", "auto", {"input": "hello", "temperature": 0.2}),
        ("vertex", "auto", {"input": "hello"}),
        (
            "bedrock",
            "auto",
            {"messages": [{"role": "user", "content": [{"text": "hello"}]}], "max_tokens": 40},
        ),
        ("ollama", "generate", {"prompt": "hello", "temperature": 0.2}),
        ("litellm", "responses", {"input": "hello"}),
    ],
)
def test_provider_contract(provider, api, input):
    calls = []

    def call(**params):
        calls.append(params)
        return {"answer": "fresh", "usage": {"input_tokens": 5, "output_tokens": 2}}

    async def async_call(**params):
        return call(**params)

    client = NS(
        chat=NS(completions=NS(create=call)),
        responses=NS(create=async_call),
        messages=NS(create=async_call),
        models=NS(generate_content=call),
        converse=call,
        generate=call,
    )
    if provider == "litellm":
        client = NS(aresponses=async_call)
    source = fixture(
        {
            **input,
            "api_key": "artifact-secret",
            "base_url": "https://attacker.invalid",
            "extra_headers": {"x": "bad"},
        }
    )
    pristine = copy.deepcopy(source)
    registry = ExecutorRegistry()
    registry.reuse_recorded("retrieval")
    registry.register_provider(client, provider=provider, api=api)
    branch = rerun(
        source, registry, from_event=source["events"][0]["id"], model="candidate", allow_live=True
    )
    assert len(calls) == 1
    assert calls[0].get("model", calls[0].get("modelId")) == "candidate"
    assert not {"api_key", "base_url", "extra_headers"} & calls[0].keys()
    if provider == "bedrock":
        assert calls[0]["inferenceConfig"]["maxTokens"] == 40
    if provider == "google":
        assert calls[0]["config"]["temperature"] == 0.2
    if provider == "ollama":
        assert calls[0]["options"]["temperature"] == 0.2
    attrs = branch["events"][1]["attributes"]
    assert attrs["total_tokens"] == 7
    assert "cost_usd" not in attrs and "stream_incomplete" not in attrs
    assert branch["events"][0]["attributes"]["reused_recorded_output"] is True
    assert source == pristine


def test_invalid_builtin_preflight_runs_no_earlier_application_callback():
    source = fixture()
    calls = []
    registry = ExecutorRegistry()
    registry.register("retrieval", lambda *_: calls.append(True))
    registry.register_provider(NS(messages=NS(create=lambda **_: None)), provider="anthropic")
    with pytest.raises(ValueError, match="max_tokens"):
        rerun(source, registry, from_event=source["events"][0]["id"], allow_live=True)
    assert calls == []


def test_sync_provider_runs_off_event_loop_and_cache_usage_is_fresh():
    from refract.provider_executor import ProviderExecutor

    def call(**params):
        with pytest.raises(RuntimeError, match="no running event loop"):
            asyncio.get_running_loop()
        return {
            "usage": {
                "input_tokens": 2,
                "output_tokens": 3,
                "cache_read_input_tokens": 4,
                "cache_creation_input_tokens": 1,
            }
        }

    executor = ProviderExecutor(
        NS(messages=NS(create=call)), provider="anthropic", defaults={"max_tokens": 20}
    )
    event = fixture()["events"][1]
    assert asyncio.run(executor(event, {}))["attributes"]["total_tokens"] == 10
