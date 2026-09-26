"""Opt-in SDK instrumentation. Provider packages are imported only on installation."""

from __future__ import annotations

import functools
import importlib
import inspect
import json
import time
from collections import deque
from collections.abc import Callable
from typing import Any

from . import Run, _current, _snapshot


def _json(value: Any) -> Any:
    if value is None or isinstance(value, (str, bool, int, float)):
        return value
    if isinstance(value, dict):
        return {str(k): _json(v) for k, v in value.items()}
    if isinstance(value, (tuple, list)):
        return [_json(v) for v in value]
    if hasattr(value, "model_dump"):
        return _json(value.model_dump(mode="json"))
    if hasattr(value, "to_dict"):
        return _json(value.to_dict())
    return {"object_type": type(value).__name__}


def _usage(payload: dict) -> dict[str, Any]:
    usage = payload.get("usage") or payload.get("usage_metadata") or {}
    result = {}
    for destination, sources in {
        "input_tokens": ("input_tokens", "prompt_tokens", "prompt_token_count", "inputTokens"),
        "output_tokens": (
            "output_tokens",
            "completion_tokens",
            "candidates_token_count",
            "outputTokens",
        ),
        "cache_read_tokens": (
            "cache_read_input_tokens",
            "cached_content_token_count",
            "cacheReadInputTokens",
        ),
        "cache_write_tokens": ("cache_creation_input_tokens", "cacheWriteInputTokens"),
    }.items():
        for source in sources:
            count = usage.get(source)
            if isinstance(count, int) and not isinstance(count, bool) and count >= 0:
                result[destination] = usage[source]
                break
    details = usage.get("input_tokens_details") or usage.get("prompt_tokens_details") or {}
    cached = details.get("cached_tokens")
    if isinstance(cached, int) and not isinstance(cached, bool) and cached >= 0:
        result["cache_read_tokens"] = details["cached_tokens"]
    thoughts = usage.get("thoughts_token_count")
    if isinstance(thoughts, int) and not isinstance(thoughts, bool) and thoughts >= 0:
        result["output_tokens"] = result.get("output_tokens", 0) + thoughts
    return result


class Instrumentation:
    def __init__(self, *, exporter=None, pricing: dict | None = None):
        self.exporter, self.pricing = exporter, pricing or {}
        self.completed_runs: deque[dict] = deque(maxlen=100)
        self.errors: deque[str] = deque(maxlen=100)
        self._patches: list[tuple[Any, str, Any, Any]] = []

    def uninstrument(self) -> None:
        """Restore methods installed by this handle, without undoing somebody else's patch."""
        for owner, name, original, wrapper in reversed(self._patches):
            if getattr(owner, name) is wrapper:
                setattr(owner, name, original)
        self._patches.clear()

    def patch(
        self,
        owner: Any,
        name: str,
        provider: str | Callable,
        *,
        model: str | None = None,
        request: Callable | None = None,
        response: Callable | None = None,
        streaming: bool = False,
        stream_key: str | None = None,
    ) -> None:
        """Observe a method without changing provider arguments or response values.

        request(args, kwargs) selects safe canonical prompt fields; response(value)
        normalizes a response/chunk for recording only. Both hooks fail open.
        """
        original = getattr(owner, name)
        if getattr(original, "__refract_instrumented__", False):
            return

        def begin(args, kwargs):
            try:
                selected = request(args, kwargs) if request else dict(kwargs)
                if model is not None:
                    selected.setdefault("model", model)
                resolved = provider(args, kwargs) if callable(provider) else provider
                return _Capture(self, resolved, selected, response=response)
            except Exception as error:
                self.errors.append(type(error).__name__)
                return None

        def finish(result, capture, kwargs):
            if capture is None:
                return result
            try:
                if streaming or kwargs.get("stream"):
                    wrapped = result[stream_key] if stream_key else result
                    proxy = (
                        _AsyncStream(wrapped, capture)
                        if hasattr(wrapped, "__aiter__")
                        else _Stream(wrapped, capture)
                    )
                    if stream_key:
                        # Preserve the returned mapping and every unrelated SDK field.
                        result[stream_key] = proxy
                        return result
                    return proxy
                capture.finish(result)
            except Exception as error:
                self.errors.append(type(error).__name__)
                capture.finish(partial=True)
            return result

        if inspect.iscoroutinefunction(inspect.unwrap(original)):

            @functools.wraps(original)
            async def asynchronous(*args, **kwargs):
                capture = begin(args, kwargs)
                try:
                    result = await original(*args, **kwargs)
                except BaseException as error:
                    if capture:
                        capture.finish(error=error)
                    raise
                return finish(result, capture, kwargs)

            wrapper = asynchronous
        else:

            @functools.wraps(original)
            def synchronous(*args, **kwargs):
                capture = begin(args, kwargs)
                try:
                    result = original(*args, **kwargs)
                except BaseException as error:
                    if capture:
                        capture.finish(error=error)
                    raise
                return finish(result, capture, kwargs)

            wrapper = synchronous
        setattr(wrapper, "__refract_instrumented__", True)
        setattr(owner, name, wrapper)
        self._patches.append((owner, name, original, wrapper))


class _Capture:
    def __init__(self, handle: Instrumentation, provider: str, kwargs: dict, *, response=None):
        self.handle, self.provider = handle, provider
        self.normalize = response or _json
        self.started = time.perf_counter()
        self.finished = False
        self.chunk_bytes = 0
        self.chunks: list[Any] = []
        self.final_response: Any = None
        self.current_usage: dict[str, Any] = {}
        self.tool_fragments: dict[tuple[int, int], dict] = {}
        self.provider_failed = False
        self.owned = _current.get() is None
        self.run = _current.get() or Run(f"{provider}.generation", exporter=handle.exporter)
        # Standalone calls get isolated runs without leaving a context token behind while streaming.
        if self.owned:
            self.run._active = True
        prompt = {
            key: _json(value)
            for key, value in kwargs.items()
            if key
            in {
                "model",
                "input",
                "messages",
                "prompt",
                "instructions",
                "system",
                "tools",
                "tool_choice",
            }
        }
        self.id = self.run.event(
            type="generation",
            name=f"{provider}/{kwargs.get('model', 'unknown')}",
            input=prompt,
            attributes={"provider": provider, "model": kwargs.get("model", "unknown")},
            status="running",
        )
        self.event = self.run.data["events"][-1]

    def chunk(self, item: Any) -> None:
        try:
            value = _json(self.normalize(item))
            if not isinstance(value, dict):
                return
            delta = value.get("delta")
            choices = value.get("choices") or []
            text_delta = isinstance(delta, str) and bool(delta)
            text_delta |= isinstance(delta, dict) and bool(delta.get("text"))
            text_delta |= any((choice.get("delta") or {}).get("content") for choice in choices)
            text_delta |= any(
                part.get("text")
                for candidate in value.get("candidates") or []
                for part in (candidate.get("content") or {}).get("parts") or []
            )
            text_delta |= bool((value.get("contentBlockDelta") or {}).get("delta", {}).get("text"))
            if text_delta and "ttft_ms" not in self.event["attributes"]:
                self.event["attributes"]["ttft_ms"] = (time.perf_counter() - self.started) * 1000
            response = (
                value.get("response") or value.get("message") or value.get("metadata") or value
            )
            self.current_usage.update(_usage(response))
            if value.get("type") in {
                "response.completed",
                "response.failed",
                "response.incomplete",
            }:
                self.final_response = response
                self.provider_failed = value.get("type") == "response.failed"
                if value.get("type") == "response.incomplete":
                    self.event["attributes"]["stream_incomplete"] = True
            if any(key.endswith("Exception") for key in value):
                self.provider_failed = True

            size = len(json.dumps(value))
            if self.chunk_bytes + size <= 1024 * 1024 and len(self.chunks) < 10000:
                self.chunks.append(value)
                self.chunk_bytes += size
                self._tool_delta(value)
            else:
                self.event["attributes"]["capture_truncated"] = True
        except Exception as error:
            self.handle.errors.append(type(error).__name__)

    def _tool_delta(self, value: dict) -> None:
        """Accumulate tool arguments split across Chat Completions/Anthropic chunks."""
        if value.get("type") == "content_block_start":
            block = value.get("content_block") or {}
            if block.get("type") == "tool_use":
                self.tool_fragments[(0, value["index"])] = dict(block, arguments="")
        elif value.get("type") == "content_block_delta":
            delta = value.get("delta") or {}
            fragment = self.tool_fragments.get((0, value.get("index", 0)))
            if fragment is not None and delta.get("type") == "input_json_delta":
                fragment["arguments"] += delta.get("partial_json", "")
        for choice in value.get("choices") or []:
            for call in (choice.get("delta") or {}).get("tool_calls") or []:
                key = (choice.get("index", 0), call["index"])
                fragment = self.tool_fragments.setdefault(
                    key, {"type": "function_call", "arguments": ""}
                )
                if call.get("id"):
                    fragment["id"] = call["id"]
                function = call.get("function") or {}
                if function.get("name"):
                    fragment["name"] = function["name"]
                fragment["arguments"] += function.get("arguments") or ""
        start = value.get("contentBlockStart") or {}
        if (start.get("start") or {}).get("toolUse"):
            tool = start["start"]["toolUse"]
            self.tool_fragments[(0, start["contentBlockIndex"])] = {
                "type": "function_call",
                "name": tool["name"],
                "id": tool["toolUseId"],
                "arguments": "",
            }
        block = value.get("contentBlockDelta") or {}
        tool_delta = (block.get("delta") or {}).get("toolUse")
        if tool_delta:
            fragment = self.tool_fragments.get((0, block.get("contentBlockIndex", 0)))
            if fragment is not None:
                fragment["arguments"] += tool_delta.get("input", "")

    def finish(self, result: Any = None, *, error: BaseException | None = None, partial=False):
        if self.finished:
            return
        self.finished = True
        try:
            output = _json(self.normalize(result)) if result is not None else self.final_response
            if output is None:
                output = {"chunks": self.chunks} if self.chunks else None
            self.event["output"] = _snapshot(output)
            attributes = self.event["attributes"]
            attributes.update(self.current_usage)
            if isinstance(output, dict):
                attributes.update(_usage(output))
                if isinstance(output.get("model"), str):
                    attributes["model"] = output["model"]
            if partial:
                attributes["stream_incomplete"] = True
            if "input_tokens" in attributes and "output_tokens" in attributes:
                total = attributes["input_tokens"] + attributes["output_tokens"]
                if self.provider in {"anthropic", "bedrock"}:
                    total += attributes.get("cache_read_tokens", 0)
                    total += attributes.get("cache_write_tokens", 0)
                attributes["total_tokens"] = total
            if "cache_read_tokens" in attributes:
                attributes["cache_hit"] = attributes["cache_read_tokens"] > 0
            if error is not None:
                attributes["exception_type"] = type(error).__name__
            prices = self.handle.pricing.get(attributes["model"])
            if prices and "input_tokens" in attributes and "output_tokens" in attributes:
                cached = attributes.get("cache_read_tokens", 0)
                uncached = attributes["input_tokens"]
                if self.provider in {"openai", "azure", "google", "vertex"}:
                    uncached = max(uncached - cached, 0)
                cost = uncached * prices["input_per_million"]
                cost += attributes["output_tokens"] * prices["output_per_million"]
                cost += cached * prices.get("cache_read_per_million", prices["input_per_million"])
                cost += attributes.get("cache_write_tokens", 0) * prices.get(
                    "cache_write_per_million", prices["input_per_million"]
                )
                attributes.update(cost_usd=cost / 1_000_000, cost_is_estimate=True)
            self.event["duration_ms"] = (time.perf_counter() - self.started) * 1000
            failed = error is not None or self.provider_failed
            self.event["status"] = "failed" if failed else "completed"
            self._tools(output)
            if self.owned:
                from . import _now

                self.run.data.update(status="failed" if failed else "completed", ended_at=_now())
                self.run._active = False
                snapshot = self.run.snapshot()
                self.handle.completed_runs.append(snapshot)
                if self.handle.exporter:
                    self.handle.exporter.submit(snapshot)
        except Exception as capture_error:
            self.handle.errors.append(type(capture_error).__name__)
            self.event.update(
                status="failed", duration_ms=(time.perf_counter() - self.started) * 1000
            )
            self.event["attributes"]["capture_error"] = type(capture_error).__name__

    def _tools(self, output: Any) -> None:
        if not isinstance(output, dict) or not self.run._active:
            return
        payload = output.get("output") or output.get("content") or []
        calls = list(payload) if isinstance(payload, list) else []
        sources = [output, *output.get("chunks", [])]
        for source in sources:
            for candidate in source.get("candidates") or []:
                for part in (candidate.get("content") or {}).get("parts") or []:
                    if part.get("function_call"):
                        function = part["function_call"]
                        calls.append(
                            {
                                "type": "function_call",
                                "name": function.get("name"),
                                "arguments": function.get("args"),
                                "id": function.get("id"),
                            }
                        )
        if isinstance(payload, dict):
            for block in (payload.get("message") or {}).get("content") or []:
                if block.get("toolUse"):
                    tool = block["toolUse"]
                    calls.append(
                        {
                            "type": "tool_use",
                            "name": tool["name"],
                            "input": tool.get("input"),
                            "id": tool.get("toolUseId"),
                        }
                    )
        if self.final_response is None:
            calls.extend(self.tool_fragments.values())
        for choice in output.get("choices") or []:
            calls.extend((choice.get("message") or {}).get("tool_calls") or [])
        for call in calls:
            if isinstance(call, dict) and call.get("type") in {
                "function_call",
                "function",
                "tool_use",
            }:
                function = call.get("function") or call
                arguments = function.get("arguments", function.get("input"))
                if isinstance(arguments, str):
                    try:
                        arguments = json.loads(arguments)
                    except ValueError:
                        pass
                self.run.event(
                    type="tool.call",
                    name=function.get("name", "tool"),
                    input=arguments,
                    parent_id=self.id,
                    attributes={"tool_call_kind": "proposal", "call_id": call.get("id")},
                    replay_policy="REQUIRES_APPROVAL",
                )
        # Supplied tool results are evidence of prior execution, not tools executed by this SDK.
        prompt = self.event["input"]
        for message in prompt.get("messages", prompt.get("input", [])) or []:
            if not isinstance(message, dict):
                continue
            content = message.get("content")
            results = content if isinstance(content, list) else []
            if message.get("role") == "tool" or message.get("type") == "function_call_output":
                results = [{"type": "tool_result", "content": content or message.get("output")}]
            for item in results:
                if isinstance(item, dict) and item.get("type") == "tool_result":
                    self.run.event(
                        type="tool.call",
                        name=message.get("name", "tool.result"),
                        output=item.get("content"),
                        parent_id=self.id,
                        attributes={"tool_call_kind": "result"},
                    )


class _Stream:
    def __init__(self, wrapped, capture: _Capture):
        self._wrapped, self._capture = wrapped, capture
        self._iterator = iter(wrapped)

    def __getattr__(self, name):
        return getattr(self._wrapped, name)

    def __iter__(self):
        return self

    def __next__(self):
        try:
            value = next(self._iterator)
        except StopIteration:
            self._capture.finish()
            raise
        except BaseException as error:
            self._capture.finish(error=error)
            raise
        self._capture.chunk(value)
        return value

    def close(self):
        try:
            close = getattr(self._wrapped, "close", None)
            if close:
                close()
        finally:
            self._capture.finish(partial=True)

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb):
        self._capture.finish(error=exc, partial=not self._capture.finished)
        self.close()


class _AsyncStream:
    def __init__(self, wrapped, capture: _Capture):
        self._wrapped, self._capture = wrapped, capture
        self._iterator = wrapped.__aiter__()

    def __getattr__(self, name):
        return getattr(self._wrapped, name)

    def __aiter__(self):
        return self

    async def __anext__(self):
        try:
            value = await self._iterator.__anext__()
        except StopAsyncIteration:
            self._capture.finish()
            raise
        except BaseException as error:
            self._capture.finish(error=error)
            raise
        self._capture.chunk(value)
        return value

    async def close(self):
        try:
            close = getattr(self._wrapped, "close", None) or getattr(self._wrapped, "aclose", None)
            if close:
                result = close()
                if inspect.isawaitable(result):
                    await result
        finally:
            self._capture.finish(partial=True)

    async def aclose(self):
        await self.close()

    async def __aenter__(self):
        return self

    async def __aexit__(self, exc_type, exc, tb):
        self._capture.finish(error=exc, partial=not self._capture.finished)
        await self.close()


def _install(provider, targets: list[tuple[str, str]], **kwargs) -> Instrumentation:
    handle = Instrumentation(**kwargs)
    try:
        for module, name in targets:
            resource = getattr(importlib.import_module(module), name)
            handle.patch(resource, "create", provider)
    except (ImportError, AttributeError) as error:
        handle.uninstrument()
        raise ImportError(
            f"Install a supported {provider} SDK before enabling instrumentation"
        ) from error
    return handle


def instrument_openai(**kwargs) -> Instrumentation:
    return _install(
        _openai_provider,
        [
            ("openai.resources.responses.responses", "Responses"),
            ("openai.resources.responses.responses", "AsyncResponses"),
            ("openai.resources.chat.completions.completions", "Completions"),
            ("openai.resources.chat.completions.completions", "AsyncCompletions"),
        ],
        **kwargs,
    )


def _openai_provider(args, kwargs):
    resource = args[0] if args else None
    client = getattr(resource, "_client", None)
    name = type(client).__name__
    return "azure" if "Azure" in name else "openai"


def instrument_azure(client=None, **kwargs) -> Instrumentation:
    """Observe AzureOpenAI globally, or one Azure/v1-compatible client explicitly."""
    if client is None:
        return instrument_openai(**kwargs)
    handle = Instrumentation(**kwargs)
    for resource in (client.responses, client.chat.completions):
        handle.patch(resource, "create", "azure")
    return handle


def _google_request(args, kwargs):
    # Config can contain callable tools or HTTP headers; record an explicit allowlist.
    config = _json(kwargs.get("config")) or {}
    result = {"model": kwargs.get("model", "unknown"), "input": kwargs.get("contents")}
    if isinstance(config, dict):
        result.update(
            {key: config[key] for key in ("tools", "system_instruction") if key in config}
        )
        if "system_instruction" in result:
            result["system"] = result.pop("system_instruction")
    return result


def instrument_google(client=None, **kwargs) -> Instrumentation:
    """Observe google-genai generate_content and generate_content_stream, sync/async.

    Supports both Gemini Developer API and Vertex AI through the same SDK.
    """
    handle = Instrumentation(**kwargs)
    try:
        provider: str | Callable
        if client is None:
            module = importlib.import_module("google.genai.models")
            owners = [module.Models, module.AsyncModels]

            def resolve_provider(args, call_kwargs):
                api = getattr(args[0], "_api_client", None)
                return "vertex" if getattr(api, "vertexai", False) else "google"

            provider = resolve_provider
        else:
            owners = [client.models, client.aio.models]
            api = getattr(client, "_api_client", None)
            provider = "vertex" if getattr(api, "vertexai", False) else "google"
        for owner in owners:
            for method in ("generate_content", "generate_content_stream"):
                handle.patch(
                    owner,
                    method,
                    provider,
                    request=_google_request,
                    streaming=method.endswith("_stream"),
                )
    except (ImportError, AttributeError) as error:
        handle.uninstrument()
        raise ImportError("Install google-genai before enabling instrumentation") from error
    return handle


def instrument_bedrock(client, **kwargs) -> Instrumentation:
    """Observe an existing boto3 bedrock-runtime client's Converse APIs.

    Credentials, region, endpoints and retry policy remain owned by the application.
    InvokeModel and async AWS clients require a custom adapter.
    """
    handle = Instrumentation(**kwargs)

    def request(args, values):
        return {
            "model": values.get("modelId", "unknown"),
            "messages": values.get("messages"),
            "system": values.get("system"),
            "tools": (values.get("toolConfig") or {}).get("tools"),
        }

    try:
        handle.patch(client, "converse", "bedrock", request=request)
        handle.patch(
            client,
            "converse_stream",
            "bedrock",
            request=request,
            streaming=True,
            stream_key="stream",
        )
    except AttributeError:
        handle.uninstrument()
        raise
    return handle


def instrument_custom(
    owner,
    method: str,
    *,
    provider: str,
    model: str | None = None,
    request: Callable | None = None,
    response: Callable | None = None,
    streaming: bool = False,
    **kwargs,
) -> Instrumentation:
    """Observe any sync/async SDK method, including local or private model clients.

    request(args, kwargs) must return canonical model/input/messages fields. response(value)
    returns a JSON-friendly recording copy (with optional usage); original values are untouched.
    For streaming methods it normalizes each chunk. Await/iteration remains application-owned.
    """
    handle = Instrumentation(**kwargs)
    handle.patch(
        owner,
        method,
        provider,
        model=model,
        request=request,
        response=response,
        streaming=streaming,
    )
    return handle


def instrument_anthropic(**kwargs) -> Instrumentation:
    return _install(
        "anthropic",
        [
            ("anthropic.resources.messages.messages", "Messages"),
            ("anthropic.resources.messages.messages", "AsyncMessages"),
        ],
        **kwargs,
    )
