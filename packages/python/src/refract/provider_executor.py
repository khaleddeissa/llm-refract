"""Replay through application-owned clients; recordings never select transport or credentials."""

from __future__ import annotations

import asyncio
import copy
import inspect
from typing import Any

from .instrumentation import _json, _usage

_COMMON = {"model", "temperature", "top_p", "tools", "tool_choice"}
_CHAT = _COMMON | {
    "messages",
    "max_tokens",
    "max_completion_tokens",
    "stop",
    "seed",
    "response_format",
    "reasoning_effort",
    "frequency_penalty",
    "presence_penalty",
    "parallel_tool_calls",
}
_RESPONSES = _COMMON | {
    "input",
    "instructions",
    "max_output_tokens",
    "reasoning",
    "parallel_tool_calls",
    "text",
}
_ANTHROPIC = _COMMON | {"messages", "system", "max_tokens", "top_k", "stop_sequences", "thinking"}
_GOOGLE = {"temperature", "top_p", "top_k", "max_output_tokens", "tools", "seed", "stop_sequences"}


class ProviderExecutor:
    """Use a configured SDK client for generation events, with trusted application defaults.

    Only prompt and generation fields are read from artifacts. Endpoints, credentials, headers,
    callbacks and retries remain controlled by the client and application-supplied defaults.
    """

    def __init__(
        self, client: Any, *, provider: str, api: str = "auto", defaults: dict | None = None
    ):
        if provider not in {
            "openai",
            "azure",
            "anthropic",
            "google",
            "vertex",
            "bedrock",
            "ollama",
            "litellm",
        }:
            raise ValueError("unsupported built-in provider; register a custom callable")
        if api not in {"auto", "chat", "responses", "generate"}:
            raise ValueError("api must be auto, chat, responses, or generate")
        self.client, self.provider, self.api = client, provider, api
        self.defaults = copy.deepcopy(defaults or {})

    def prepare(self, event: dict):
        if event["type"] != "generation":
            raise ValueError("provider executors accept generation events only")
        source = event["input"] if isinstance(event["input"], dict) else {"input": event["input"]}
        source = copy.deepcopy(source)
        model = (
            event["attributes"].get("model") or self.defaults.get("model") or source.get("model")
        )
        if not isinstance(model, str) or not model:
            raise ValueError("generation model is required")
        provider = self.provider
        text = source.get("input", source.get("prompt", ""))
        if provider in {"openai", "azure", "litellm"}:
            responses = self.api == "responses" or (
                self.api == "auto" and "input" in source and "messages" not in source
            )
            if responses:
                params = {k: v for k, v in source.items() if k in _RESPONSES}
                params.setdefault("input", text)
                method = (
                    self.client.responses.create
                    if provider != "litellm"
                    else (getattr(self.client, "aresponses", None) or self.client.responses)
                )
            else:
                params = {k: v for k, v in source.items() if k in _CHAT}
                params.setdefault("messages", [{"role": "user", "content": text}])
                if source.get("system") is not None:
                    params["messages"].insert(0, {"role": "system", "content": source["system"]})
                method = (
                    self.client.chat.completions.create
                    if provider != "litellm"
                    else (getattr(self.client, "acompletion", None) or self.client.completion)
                )
            params["stream"] = False
        elif provider == "anthropic":
            params = {k: v for k, v in source.items() if k in _ANTHROPIC}
            params.setdefault("messages", [{"role": "user", "content": text}])
            method = self.client.messages.create
        elif provider in {"google", "vertex"}:
            config = {k: v for k, v in source.items() if k in _GOOGLE}
            if source.get("system") is not None:
                config["system_instruction"] = source["system"]
            params = {
                "contents": source.get(
                    "input", source.get("contents", source.get("messages", text))
                ),
                "config": config,
            }
            method = self.client.models.generate_content
        elif provider == "bedrock":
            inference = {
                target: source[key]
                for key, target in [
                    ("temperature", "temperature"),
                    ("top_p", "topP"),
                    ("max_tokens", "maxTokens"),
                    ("stop_sequences", "stopSequences"),
                ]
                if key in source
            }
            params = {
                "messages": source.get("messages", [{"role": "user", "content": [{"text": text}]}])
            }
            if source.get("system") is not None:
                params["system"] = source["system"]
            if inference:
                params["inferenceConfig"] = inference
            if source.get("tools") is not None:
                params["toolConfig"] = {"tools": source["tools"]}
                if source.get("tool_choice") is not None:
                    params["toolConfig"]["toolChoice"] = source["tool_choice"]
            method = self.client.converse
        else:
            generation = self.api == "generate" or (self.api == "auto" and "messages" not in source)
            params = {"prompt": text} if generation else {"messages": source["messages"]}
            options = {
                k: v
                for k, v in source.items()
                if k in {"temperature", "top_p", "top_k", "seed", "stop"}
            }
            if options:
                params["options"] = options
            if source.get("tools") is not None and not generation:
                params["tools"] = source["tools"]
            params["stream"] = False
            method = self.client.generate if generation else self.client.chat
        params.update(copy.deepcopy(self.defaults))
        params["modelId" if provider == "bedrock" else "model"] = model
        if provider in {"openai", "azure", "litellm", "ollama"}:
            params["stream"] = False
        if provider == "anthropic" and "max_tokens" not in params:
            raise ValueError("Anthropic rerun requires a recorded or configured max_tokens value")
        if not callable(method):
            raise ValueError("configured provider method is not callable")
        return method, params

    def validate(self, event: dict) -> None:
        """Validate the configured method and required model/options before any suffix executes."""
        self.prepare(event)

    async def __call__(self, event: dict, context: dict) -> dict:
        method, params = self.prepare(event)
        # Sync SDKs must not block an application's event loop. SDK timeouts remain client-owned.
        result = (
            method(**params)
            if inspect.iscoroutinefunction(method)
            else await asyncio.to_thread(method, **params)
        )
        if inspect.isawaitable(result):
            result = await result
        output = _json(result)
        attributes: dict = {
            "provider": self.provider,
            "model": params.get("model", params.get("modelId")),
        }
        if isinstance(output, dict):
            attributes.update(_usage(output))
            if self.provider == "ollama":
                for key, target in [
                    ("prompt_eval_count", "input_tokens"),
                    ("eval_count", "output_tokens"),
                ]:
                    count = output.get(key)
                    if isinstance(count, int) and not isinstance(count, bool) and count >= 0:
                        attributes[target] = count
            if "input_tokens" in attributes and "output_tokens" in attributes:
                total = attributes["input_tokens"] + attributes["output_tokens"]
                if self.provider in {"anthropic", "bedrock"}:
                    total += attributes.get("cache_read_tokens", 0) + attributes.get(
                        "cache_write_tokens", 0
                    )
                attributes["total_tokens"] = total
        return {"output": output, "attributes": attributes}
