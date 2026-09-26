"""Manual, provider-neutral instrumentation with local recording by default."""

from __future__ import annotations

import contextvars
import copy
import functools
import inspect
import json
import math
import time
import urllib.request
import uuid
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, Self

from .artifact import pack

SPEC_VERSION = "refract.execution.v1"
_current: contextvars.ContextVar[Run | None] = contextvars.ContextVar("refract_run", default=None)
_parent: contextvars.ContextVar[tuple[str, str] | None] = contextvars.ContextVar(
    "refract_parent", default=None
)
_METRICS = {
    "input_tokens",
    "output_tokens",
    "cache_read_tokens",
    "cache_write_tokens",
    "total_tokens",
}
_TYPES = {
    "generation",
    "tool.call",
    "retrieval",
    "decision",
    "state.change",
    "checkpoint",
    "handoff",
    "human",
    "artifact",
    "error",
}
_POLICIES = {"READ_ONLY", "MOCK", "RECORDED", "LIVE", "REQUIRES_APPROVAL", "BLOCKED"}


def _now() -> str:
    return datetime.now(UTC).isoformat()


def _redact(value: Any) -> Any:
    if isinstance(value, dict):
        return {
            k: "[REDACTED]"
            if not (k in _METRICS and isinstance(v, int) and not isinstance(v, bool) and v >= 0)
            and any(
                s in k.lower().replace("-", "_")
                for s in (
                    "password",
                    "secret",
                    "token",
                    "api_key",
                    "authorization",
                    "cookie",
                    "email",
                )
            )
            else _redact(v)
            for k, v in value.items()
        }
    if isinstance(value, list):
        return [_redact(v) for v in value]
    return value


def _snapshot(value: Any) -> Any:
    return _redact(json.loads(json.dumps(value, allow_nan=False)))


class Run:
    def __init__(
        self,
        name: str,
        *,
        path: str | Path | None = None,
        metadata: dict | None = None,
        endpoint: str | None = None,
        exporter: Any = None,
        fail_open: bool = True,
        api_key: str | None = None,
    ):
        if not name.strip():
            raise ValueError("run name cannot be empty")
        self.path, self.endpoint = path, endpoint
        self.exporter, self.fail_open, self.api_key = exporter, fail_open, api_key
        self.recording_errors: list[str] = []
        self.data: dict[str, Any] = {
            "spec_version": SPEC_VERSION,
            "id": f"run_{uuid.uuid4()}",
            "name": name,
            "status": "running",
            "started_at": _now(),
            "ended_at": None,
            "metadata": _snapshot(metadata or {}),
            "events": [],
        }
        self._active = False

    def __enter__(self) -> Self:
        if self._active or self.data["status"] != "running":
            raise RuntimeError("run contexts are single-use")
        self._active = True
        self._token = _current.set(self)
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        try:
            if exc is not None:
                self.event(
                    type="error",
                    name=type(exc).__name__,
                    status="failed",
                    output={"exception_type": type(exc).__name__},
                )
            self.data["status"] = "failed" if exc is not None else "completed"
            self.data["ended_at"] = _now()
            if self.path:
                self.export(self.path)
            if self.endpoint:
                self.send(self.endpoint, api_key=self.api_key)
            if self.exporter:
                self.exporter.submit(self.snapshot())
        except Exception as recording_error:
            self.recording_errors.append(type(recording_error).__name__)
            if exc is None and not self.fail_open:
                raise
            if exc is not None:
                exc.add_note(f"Refract recording also failed: {type(recording_error).__name__}")
        finally:
            self._active = False
            _current.reset(self._token)

    def event(
        self,
        *,
        type: str,
        name: str,
        input: Any = None,
        output: Any = None,
        parent_id: str | None = None,
        duration_ms: float = 0,
        attributes: dict | None = None,
        replay_policy: str = "RECORDED",
        status: str = "completed",
    ) -> str:
        if not self._active:
            raise RuntimeError("events require an active run")
        if type not in _TYPES or replay_policy not in _POLICIES:
            raise ValueError("unsupported event type or replay policy")
        if status not in {"running", "completed", "failed"} or not name.strip():
            raise ValueError("invalid event name/status")
        if not math.isfinite(duration_ms) or duration_ms < 0:
            raise ValueError("duration must be finite and nonnegative")
        active_parent = _parent.get()
        if parent_id is None and active_parent and active_parent[0] == self.data["id"]:
            parent_id = active_parent[1]
        if parent_id and parent_id not in {e["id"] for e in self.data["events"]}:
            raise ValueError("parent must precede child")
        event = _snapshot(
            {
                "id": f"evt_{uuid.uuid4()}",
                "run_id": self.data["id"],
                "parent_id": parent_id,
                "type": type,
                "name": name,
                "timestamp": _now(),
                "duration_ms": duration_ms,
                "status": status,
                "input": input,
                "output": output,
                "attributes": attributes or {},
                "replay_policy": replay_policy,
            }
        )
        self.data["events"].append(event)
        return event["id"]

    def export(self, path: str | Path) -> None:
        path = Path(path)
        data = _snapshot(self.data)
        body = pack(data) if path.suffix == ".rfr" else json.dumps(data, indent=2).encode()
        with path.open("xb") as target:
            target.write(body)

    def send(self, endpoint: str, *, api_key: str | None = None) -> None:
        headers = {"Content-Type": "application/json"}
        if api_key:
            headers["Authorization"] = f"Bearer {api_key}"
        request = urllib.request.Request(
            endpoint.rstrip("/") + "/v1/runs",
            json.dumps(_snapshot(self.data)).encode(),
            headers,
            method="POST",
        )
        with urllib.request.urlopen(request, timeout=10) as response:
            response.read()

    def snapshot(self) -> dict:
        return copy.deepcopy(self.data)


def run(name: str, **kwargs) -> Run:
    return Run(name, **kwargs)


def event(**kwargs) -> str:
    current = _current.get()
    if current is None:
        raise RuntimeError("refract.event requires an active refract.run context")
    return current.event(**kwargs)


class Span:
    """An execution event opened before its children and completed on context exit."""

    def __init__(self, *, type: str, name: str, **kwargs):
        self.kwargs = {"type": type, "name": name, **kwargs}

    def __enter__(self) -> Self:
        current = _current.get()
        if current is None:
            raise RuntimeError("spans require an active run")
        self.run = current
        self.id = current.event(**self.kwargs, status="running")
        self.data = current.data["events"][-1]
        self.started = time.perf_counter()
        self.token = _parent.set((current.data["id"], self.id))
        return self

    def output(self, value: Any) -> None:
        self.data["output"] = _snapshot(value)

    def attributes(self, **values: Any) -> None:
        self.data["attributes"].update(_snapshot(values))

    def __exit__(self, exc_type, exc, tb) -> None:
        try:
            self.data["status"] = "failed" if exc is not None else "completed"
            self.data["duration_ms"] = (time.perf_counter() - self.started) * 1000
            if exc is not None:
                self.data["attributes"]["exception_type"] = type(exc).__name__
        finally:
            _parent.reset(self.token)


def span(*, type: str, name: str, **kwargs) -> Span:
    return Span(type=type, name=name, **kwargs)


def instrument_openai(**kwargs):
    """Opt in to capturing installed OpenAI SDK create calls; returns an undo handle."""
    from .instrumentation import instrument_openai as install

    return install(**kwargs)


def instrument_anthropic(**kwargs):
    """Opt in to capturing installed Anthropic SDK create calls; returns an undo handle."""
    from .instrumentation import instrument_anthropic as install

    return install(**kwargs)


def instrument_google(client=None, **kwargs):
    """Capture Gemini and Vertex AI google-genai content generation calls."""
    from .instrumentation import instrument_google as install

    return install(client, **kwargs)


def instrument_azure(client=None, **kwargs):
    """Capture Azure OpenAI Responses/Chat Completions on a client or globally."""
    from .instrumentation import instrument_azure as install

    return install(client, **kwargs)


def instrument_bedrock(client, **kwargs):
    """Capture boto3 Converse/ConverseStream on one existing Bedrock client."""
    from .instrumentation import instrument_bedrock as install

    return install(client, **kwargs)


def instrument_custom(owner, method: str, *, provider: str, **kwargs):
    """Capture an arbitrary SDK method using application-owned normalization hooks."""
    from .instrumentation import instrument_custom as install

    return install(owner, method, provider=provider, **kwargs)


def trace(fn=None, *, name: str | None = None):
    """Trace sync/async entrypoints. Arguments and exception messages are not auto-captured."""

    def decorate(func):
        if inspect.iscoroutinefunction(func):

            @functools.wraps(func)
            async def asynchronous(*args, **kwargs):
                with run(name or func.__name__):
                    start = time.perf_counter()
                    result = await func(*args, **kwargs)
                    event(
                        type="decision",
                        name=func.__name__,
                        duration_ms=(time.perf_counter() - start) * 1000,
                    )
                    return result

            return asynchronous

        @functools.wraps(func)
        def synchronous(*args, **kwargs):
            with run(name or func.__name__):
                start = time.perf_counter()
                result = func(*args, **kwargs)
                event(
                    type="decision",
                    name=func.__name__,
                    duration_ms=(time.perf_counter() - start) * 1000,
                )
                return result

        return synchronous

    return decorate(fn) if fn else decorate
