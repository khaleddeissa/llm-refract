import asyncio
from types import SimpleNamespace

import pytest

import refract


@pytest.mark.parametrize("streaming", [False, True])
@pytest.mark.parametrize("asynchronous", [False, True])
def test_nested_gateway_provider_is_one_generation(streaming, asynchronous):
    response = {
        "choices": [{"message": {"content": "answer"}}],
        "usage": {"prompt_tokens": 3, "completion_tokens": 2},
    }

    def provider_call(**kwargs):
        if streaming:
            return iter(
                [{"choices": [{"delta": {"content": "answer"}}], "usage": response["usage"]}]
            )
        return response

    async def async_provider(**kwargs):
        if streaming:

            async def chunks():
                for chunk in provider_call(**kwargs):
                    yield chunk

            return chunks()
        await asyncio.sleep(0)
        return response

    provider = SimpleNamespace(create=async_provider if asynchronous else provider_call)
    inner = refract.instrument_custom(provider, "create", provider="openai")
    gateway = (
        SimpleNamespace(acompletion=provider.create)
        if asynchronous
        else SimpleNamespace(completion=provider.create)
    )
    # Avoid copying the marked wrapper: a real gateway delegates from its own function.
    if asynchronous:

        async def delegate(**kwargs):
            return await provider.create(**kwargs)

        gateway.acompletion = delegate
    else:
        gateway.completion = lambda **kwargs: provider.create(**kwargs)
    outer = refract.instrument_litellm(gateway)

    async def exercise():
        result = await gateway.acompletion(model="test", messages=[], stream=streaming)
        if streaming:
            async for _ in result:
                pass

    try:
        with refract.run("nested") as run:
            if asynchronous:
                asyncio.run(exercise())
            else:
                result = gateway.completion(model="test", messages=[], stream=streaming)
                if streaming:
                    list(result)
        generations = [e for e in run.data["events"] if e["type"] == "generation"]
        assert len(generations) == 1
        assert generations[0]["attributes"]["total_tokens"] == 5
        assert generations[0]["attributes"]["provider"] == "litellm"
        assert not inner.errors and not outer.errors
    finally:
        outer.uninstrument()
        inner.uninstrument()


def test_nested_context_is_reset_after_failure_and_between_concurrent_calls():
    async def completion(**kwargs):
        await asyncio.sleep(0)
        if kwargs["model"] == "bad":
            raise ValueError("fixture")
        return {"usage": {"prompt_tokens": 1, "completion_tokens": 2}}

    provider = SimpleNamespace(create=completion)
    inner = refract.instrument_custom(provider, "create", provider="fixture")

    async def gateway_call(**kwargs):
        return await provider.create(**kwargs)

    gateway = SimpleNamespace(acompletion=gateway_call)
    outer = refract.instrument_litellm(gateway)

    async def exercise():
        with refract.run("concurrent") as run:
            await asyncio.gather(
                gateway.acompletion(model="bad"),
                gateway.acompletion(model="good"),
                return_exceptions=True,
            )
            await provider.create(model="direct")
        return run

    try:
        run = asyncio.run(exercise())
        assert len(run.data["events"]) == 3
        assert sum(e["status"] == "failed" for e in run.data["events"]) == 1
    finally:
        outer.uninstrument()
        inner.uninstrument()


def test_library_adapters_preserve_positional_model_and_responses_input():
    client = SimpleNamespace(
        responses=lambda input, model: {"output": input},
        completion=lambda model, messages: {"choices": []},
    )
    handle = refract.instrument_litellm(client)
    try:
        with refract.run("arguments") as run:
            client.responses(input="preserve this", model="fixture")
            client.completion("fixture", [{"role": "user", "content": "hello"}])
        assert run.data["events"][0]["input"]["input"] == "preserve this"
        assert run.data["events"][1]["attributes"]["model"] == "fixture"
        assert run.data["events"][1]["input"]["messages"][0]["content"] == "hello"
    finally:
        handle.uninstrument()
