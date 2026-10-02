"""Optional adapters for existing provider clients; credentials stay with the application."""

from __future__ import annotations

import json
from typing import Any

from refract.instrumentation import Instrumentation, _json


def _body(value: Any) -> dict:
    if isinstance(value, (str, bytes, bytearray)):
        result = json.loads(value)
        if not isinstance(result, dict):
            raise ValueError("model body must be an object")
        return result
    if isinstance(value, dict):
        return value
    # Never consume a caller-owned file/stream to inspect its request.
    return {"body_not_captured": True}


def _bedrock_response(value):
    data = _json(value) if not isinstance(value, dict) else value
    if (chunk := data.get("chunk")) and isinstance(chunk, dict):
        data = _body(chunk.get("bytes", b"{}"))
    usage = dict(data.get("usage") or {})
    # Native model families use different response envelopes.
    aliases = {
        "inputTokenCount": "input_tokens",
        "prompt_token_count": "input_tokens",
        "generation_token_count": "output_tokens",
    }
    for source, target in aliases.items():
        if source in data:
            usage[target] = data[source]
    metrics = data.get("amazon-bedrock-invocationMetrics") or {}
    for source, target in (
        ("inputTokenCount", "input_tokens"),
        ("outputTokenCount", "output_tokens"),
    ):
        if source in metrics:
            usage[target] = metrics[source]
    if data.get("results"):
        usage["output_tokens"] = sum(item.get("tokenCount", 0) for item in data["results"])
    return {**data, "usage": usage}


class _ObservedBody:
    """Observe bytes as the application reads them; never eagerly drain a response."""

    def __init__(self, body, capture):
        self.body, self.capture = body, capture
        self.parts = bytearray()
        self.truncated = False

    def __getattr__(self, name):
        return getattr(self.body, name)

    def _observe(self, data, complete):
        if len(self.parts) + len(data) <= 1024 * 1024:
            self.parts.extend(data)
        else:
            self.truncated = True
        if complete:
            if self.truncated:
                self.capture.event["attributes"]["capture_truncated"] = True
                self.capture.finish(partial=True)
            else:
                try:
                    value = _body(self.parts)
                except (ValueError, TypeError):
                    self.capture.finish(partial=True)
                else:
                    self.capture.finish(value)
        return data

    def read(self, amt=None, *args, **kwargs):
        import inspect

        try:
            value = self.body.read(amt, *args, **kwargs)
        except BaseException as error:
            self.capture.finish(error=error)
            raise
        if inspect.isawaitable(value):

            async def resolve():
                try:
                    data = await value
                    return self._observe(data, amt is None or (amt != 0 and not data))
                except BaseException as error:
                    self.capture.finish(error=error)
                    raise

            return resolve()
        return self._observe(value, amt is None or (amt != 0 and not value))

    def readinto(self, buffer):
        data = self.read(len(buffer))
        buffer[: len(data)] = data
        return len(data)

    def iter_chunks(self, chunk_size=1024):
        while chunk := self.read(chunk_size):
            yield chunk

    def __iter__(self):
        return self.iter_chunks()

    def close(self):
        self.capture.finish(partial=True)
        return self.body.close()

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.close()

    async def __aenter__(self):
        return self

    async def __aexit__(self, *args):
        import inspect

        result = self.close()
        if inspect.isawaitable(result):
            await result


def instrument_bedrock_native(client, **options) -> Instrumentation:
    """Capture InvokeModel and InvokeModelWithResponseStream on boto3/aiobotocore clients."""
    import functools
    import inspect

    from refract.instrumentation import _AsyncStream, _Capture, _nested_generation, _Stream

    handle = Instrumentation(**options)
    for name in ("invoke_model", "invoke_model_with_response_stream"):
        original = getattr(client, name, None)
        if original is None or getattr(original, "__refract_instrumented__", False):
            continue

        def install(method, streaming):
            @functools.wraps(method)
            def wrapper(*args, **kwargs):
                if _nested_generation():
                    return method(*args, **kwargs)
                capture = None
                try:
                    request = _body(kwargs.get("body", {}))
                    capture = _Capture(
                        handle,
                        "bedrock",
                        {**request, "model": kwargs.get("modelId", "unknown")},
                        response=_bedrock_response,
                    )
                except Exception as error:
                    handle.errors.append(type(error).__name__)

                def observe(result):
                    if capture is not None:
                        body = result.get("body")
                        if body is not None:
                            if streaming:
                                result["body"] = (
                                    _AsyncStream(body, capture)
                                    if hasattr(body, "__aiter__")
                                    else _Stream(body, capture)
                                )
                            else:
                                result["body"] = _ObservedBody(body, capture)
                        else:
                            capture.finish(partial=True)
                    return result

                try:
                    result = method(*args, **kwargs)
                except BaseException as error:
                    if capture:
                        capture.finish(error=error)
                    raise
                if inspect.isawaitable(result):

                    async def resolve():
                        try:
                            return observe(await result)
                        except BaseException as error:
                            if capture:
                                capture.finish(error=error)
                            raise

                    return resolve()
                return observe(result)

            setattr(wrapper, "__refract_instrumented__", True)
            return wrapper

        wrapper = install(original, name.endswith("stream"))
        setattr(client, name, wrapper)
        handle._patches.append((client, name, original, wrapper))
    if not handle._patches:
        raise ValueError("client has no uninstrumented native Bedrock methods")
    return handle


def instrument_vertex(model, **options) -> Instrumentation:
    """Observe an existing vertexai.generative_models.GenerativeModel instance."""
    handle = Instrumentation(**options)

    def request(args, kwargs):
        return {
            "model": str(getattr(model, "_model_name", "vertex-model")),
            "input": kwargs.get("contents", args[0] if args else None),
        }

    try:
        for name in ("generate_content", "generate_content_async"):
            if hasattr(model, name):
                handle.patch(model, name, "vertex", request=request)
        if not handle._patches:
            raise ValueError("unsupported Vertex model")
    except Exception:
        handle.uninstrument()
        raise
    return handle


def instrument_library(client, library: str, **options) -> Instrumentation:
    """Select known method/request adapters without requiring caller-written normalizers.

    Names are explicit, rather than guessed from credentials or network endpoints.
    """
    methods = {
        "ollama": ("chat", "generate"),
        "huggingface": ("chat_completion", "text_generation"),
        "llama_cpp": ("create_chat_completion", "create_completion"),
        "litellm": ("completion", "acompletion", "responses", "aresponses"),
    }
    if library not in methods:
        raise ValueError(f"unsupported library: {library}")
    owner = client
    handle = Instrumentation(**options)

    def response(value):
        data = _json(value)
        if library == "ollama" and isinstance(data, dict):
            usage = {
                key: data[source]
                for key, source in (
                    ("input_tokens", "prompt_eval_count"),
                    ("output_tokens", "eval_count"),
                )
                if source in data
            }
            return {**data, "usage": usage}
        return data

    try:
        for method in methods[library]:
            if callable(getattr(owner, method, None)):
                import inspect

                signature = inspect.signature(getattr(owner, method))

                def request(args, kwargs, signature=signature):
                    # Bound signatures preserve positional model/prompt arguments and keyword input.
                    values = dict(signature.bind_partial(*args, **kwargs).arguments)
                    values.update(values.pop("kwargs", {}))
                    values.update(kwargs)
                    values.setdefault("model", getattr(owner, "model", library))
                    if "input" not in values and "prompt" in values:
                        values["input"] = values["prompt"]
                    return values

                handle.patch(owner, method, library, request=request, response=response)
        if not handle._patches:
            raise ValueError(f"no supported {library} methods")
    except Exception:
        handle.uninstrument()
        raise
    return handle
