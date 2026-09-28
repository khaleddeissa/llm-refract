"""Opt-in observation of an existing Realtime connection's recv method."""

from __future__ import annotations

import functools
import inspect

from refract.instrumentation import Instrumentation, _Capture, _json


def _without_audio(value):
    if isinstance(value, dict):
        return {
            key: _without_audio(item)
            for key, item in value.items()
            if key not in {"audio", "audio_data", "audio_bytes"}
        }
    if isinstance(value, list):
        return [_without_audio(item) for item in value]
    return value


def instrument_realtime(connection, *, provider="openai", model="realtime", **options):
    """Observe recv() calls, including SDK iteration that delegates to recv().

    The caller owns authentication, session configuration and connection lifecycle.
    One generation is recorded per response ID. Binary audio is never retained.
    """

    class RealtimeInstrumentation(Instrumentation):
        def uninstrument(self) -> None:
            incomplete()
            super().uninstrument()

    handle = RealtimeInstrumentation(**options)
    original = connection.recv
    if getattr(original, "__refract_instrumented__", False):
        raise ValueError("connection is already instrumented")
    pending: dict[str, _Capture] = {}

    def observe(event):
        try:
            value = _json(event)
            kind = value.get("type")
            response = value.get("response") or {}
            identity = response.get("id") or value.get("response_id")
            if not isinstance(identity, str) or not identity or len(identity) > 256:
                return event
            if kind == "response.created" and identity not in pending:
                if len(pending) >= 32:
                    raise ValueError("too many concurrent realtime responses")
                pending[identity] = _Capture(handle, provider, {"model": model})
            capture = pending.get(identity)
            if capture is None:
                return event
            if kind == "response.done":
                pending.pop(identity)
                status = response.get("status")
                capture.provider_failed = status == "failed"
                capture.finish(_without_audio(response), partial=status != "completed")
            elif kind in {
                "response.output_text.delta",
                "response.text.delta",
                "response.audio_transcript.delta",
                "response.output_audio_transcript.delta",
            }:
                capture.chunk(value)
        except Exception as error:
            handle.errors.append(type(error).__name__)
        return event

    def incomplete(error=None):
        for capture in pending.values():
            capture.finish(error=error, partial=True)
        pending.clear()

    @functools.wraps(original)
    def recv(*args, **kwargs):
        try:
            result = original(*args, **kwargs)
        except BaseException as error:
            incomplete(error)
            raise
        if inspect.isawaitable(result):

            async def resolve():
                try:
                    return observe(await result)
                except BaseException as error:
                    incomplete(error)
                    raise

            return resolve()
        return observe(result)

    setattr(recv, "__refract_instrumented__", True)
    connection.recv = recv
    handle._patches.append((connection, "recv", original, recv))
    if callable(getattr(connection, "close", None)):
        close = connection.close

        @functools.wraps(close)
        def closing(*args, **kwargs):
            incomplete()
            return close(*args, **kwargs)

        connection.close = closing
        handle._patches.append((connection, "close", close, closing))
    return handle
