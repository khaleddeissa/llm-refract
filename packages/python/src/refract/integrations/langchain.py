"""Optional LangChain callbacks for chains, chat/LLM calls, retrievers, and tools."""

from __future__ import annotations

import time

from refract import _current, _snapshot
from refract.instrumentation import _json, _usage


def langchain_handler(*, provider: str | None = None):
    """Construct inside a refract.run and pass via config={'callbacks': [handler]}.

    langchain-core is an optional application dependency. The handler records actual
    tool execution separately from provider-generated tool call proposals.
    """
    try:
        from langchain_core.callbacks.base import BaseCallbackHandler
    except ImportError as error:
        raise ImportError(
            "Install langchain-core to use the LangChain callback integration"
        ) from error
    active = _current.get()
    if active is None:
        raise RuntimeError("construct langchain_handler inside a refract.run context")
    recording = active

    class RefractCallbackHandler(BaseCallbackHandler):
        run_inline = True
        raise_error = False

        def __init__(self):
            self.spans: dict[str, tuple[dict, float]] = {}
            self.errors: list[str] = []

        def _start(self, kind, serialized, payload, run_id, parent_run_id=None, **kwargs):
            try:
                parent = self.spans.get(str(parent_run_id))
                params = kwargs.get("invocation_params") or {}
                name = kwargs.get("name") or (serialized or {}).get("name") or kind
                attributes = {"framework": "langchain"}
                if provider:
                    attributes["provider"] = provider
                model = params.get("model_name") or params.get("model")
                if model:
                    attributes["model"] = model
                recording.event(
                    type=kind,
                    name=name,
                    input=_json(payload),
                    parent_id=parent[0]["id"] if parent else None,
                    status="running",
                    attributes=attributes,
                    replay_policy="REQUIRES_APPROVAL" if kind == "tool.call" else "RECORDED",
                )
                self.spans[str(run_id)] = (recording.data["events"][-1], time.perf_counter())
            except Exception as error:
                self.errors.append(type(error).__name__)

        def _end(self, result, run_id, error=None):
            try:
                event, started = self.spans[str(run_id)]
                event["duration_ms"] = (time.perf_counter() - started) * 1000
                event["status"] = "failed" if error is not None else "completed"
                if error is not None:
                    event["attributes"]["exception_type"] = type(error).__name__
                else:
                    value = _json(result)
                    if isinstance(value, dict) and not isinstance(result, dict):
                        # LLMResult declares base Generation/BaseMessage types. Pydantic's
                        # nested dump omits AIMessage usage and tool calls unless we serialize
                        # the actual message instance, rather than its declared base type.
                        for originals, serialized in zip(
                            getattr(result, "generations", []), value.get("generations", [])
                        ):
                            for original, generation in zip(originals, serialized):
                                message = getattr(original, "message", None)
                                if message is not None and isinstance(generation, dict):
                                    generation["message"] = _json(message)
                    event["output"] = _snapshot(value)
                    if isinstance(value, dict):
                        details = value.get("llm_output") or {}
                        usage = details.get("token_usage") or details.get("usage") or {}
                        event["attributes"].update(_usage({"usage": usage}))
                        if details.get("model_name"):
                            event["attributes"]["model"] = details["model_name"]
                        # Chat providers commonly attach usage to AIMessage rather than llm_output.
                        messages = [
                            generation.get("message") or {}
                            for group in value.get("generations") or []
                            for generation in group
                            if isinstance(generation, dict)
                        ]
                        counts: dict[str, int] = {}
                        for message in messages:
                            for key, count in _usage(message).items():
                                counts[key] = counts.get(key, 0) + count
                        for key, count in counts.items():
                            event["attributes"].setdefault(key, count)
            except Exception as failure:
                self.errors.append(type(failure).__name__)

        def on_chain_start(self, serialized, inputs, *, run_id, parent_run_id=None, **kwargs):
            self._start("decision", serialized, inputs, run_id, parent_run_id, **kwargs)

        def on_chain_end(self, outputs, *, run_id, **kwargs):
            self._end(outputs, run_id)

        def on_chain_error(self, error, *, run_id, **kwargs):
            self._end(None, run_id, error)

        def on_llm_start(self, serialized, prompts, *, run_id, parent_run_id=None, **kwargs):
            self._start("generation", serialized, prompts, run_id, parent_run_id, **kwargs)

        def on_chat_model_start(
            self, serialized, messages, *, run_id, parent_run_id=None, **kwargs
        ):
            self._start("generation", serialized, messages, run_id, parent_run_id, **kwargs)

        def on_llm_new_token(self, token, *, run_id, **kwargs):
            try:
                event, started = self.spans[str(run_id)]
                if token:
                    event["attributes"].setdefault(
                        "ttft_ms", (time.perf_counter() - started) * 1000
                    )
            except Exception as error:
                self.errors.append(type(error).__name__)

        def on_llm_end(self, response, *, run_id, **kwargs):
            self._end(response, run_id)

        def on_llm_error(self, error, *, run_id, **kwargs):
            self._end(None, run_id, error)

        def on_tool_start(self, serialized, input_str, *, run_id, parent_run_id=None, **kwargs):
            self._start("tool.call", serialized, input_str, run_id, parent_run_id, **kwargs)

        def on_tool_end(self, output, *, run_id, **kwargs):
            self._end(output, run_id)

        def on_tool_error(self, error, *, run_id, **kwargs):
            self._end(None, run_id, error)

        def on_retriever_start(self, serialized, query, *, run_id, parent_run_id=None, **kwargs):
            self._start("retrieval", serialized, query, run_id, parent_run_id, **kwargs)

        def on_retriever_end(self, documents, *, run_id, **kwargs):
            self._end(documents, run_id)

        def on_retriever_error(self, error, *, run_id, **kwargs):
            self._end(None, run_id, error)

    return RefractCallbackHandler()
