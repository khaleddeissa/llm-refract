"""Application-owned executable branches. No code is loaded from artifact contents."""

from __future__ import annotations

import argparse
import asyncio
import copy
import importlib
import inspect
import json
import sys
import time
import uuid
from collections.abc import Callable
from pathlib import Path
from typing import Any

from . import _POLICIES, _now, _snapshot
from .artifact import pack, unpack


class ExecutorRegistry:
    def __init__(self):
        self.executors: dict[str, Callable] = {}

    def register(self, name: str, executor: Callable) -> None:
        if not callable(executor):
            raise TypeError("executor must be callable")
        self.executors[name] = executor

    def resolve(self, event: dict) -> Callable:
        callback = (
            self.executors.get(event["name"])
            or self.executors.get(event["type"])
            or self.executors.get("*")
        )
        if callback is None:
            raise ValueError(f"no executor registered for {event['name']}")
        return callback


def _pointer(value: Any, path: str) -> Any:
    if path == "":
        return copy.deepcopy(value)
    if not path.startswith("/"):
        raise ValueError("output binding paths must be JSON pointers")
    for part in path[1:].split("/"):
        part = part.replace("~1", "/").replace("~0", "~")
        value = value[int(part)] if isinstance(value, list) else value[part]
    return copy.deepcopy(value)


def _bind(event: dict, context: dict) -> None:
    outputs = {e["id"]: e["output"] for e in context["events"]}
    for key, binding in event["attributes"].get("input_bindings", {}).items():
        if not isinstance(event["input"], dict):
            raise ValueError("input bindings require object input")
        event["input"][key] = _pointer(outputs[binding["event_id"]], binding.get("path", ""))


async def rerun_async(
    recording: dict,
    registry: ExecutorRegistry,
    *,
    from_event: str,
    model: str | None = None,
    replace_models: dict[str, str] | None = None,
    allow_live: bool = False,
    approved_events: set[str] | None = None,
) -> dict:
    """Rerun a suffix in recorded order using explicitly registered application code.

    Parent links express containment, not general program control flow. Bindings explicitly
    connect new outputs to subsequent inputs. This does not restore Python stack/heap state.
    """
    if not allow_live:
        raise PermissionError("executable rerun requires allow_live=True")
    if recording.get("spec_version") != "refract.execution.v1":
        raise ValueError("unsupported execution version")
    positions = [i for i, event in enumerate(recording["events"]) if event["id"] == from_event]
    if len(positions) != 1:
        raise ValueError("from_event must identify exactly one event")
    start = positions[0]
    approved = approved_events or set()
    # Resolve and authorize the entire suffix before any application callback executes.
    callbacks = []
    known = set()
    for index, event in enumerate(recording["events"]):
        if event["id"] in known or (event.get("parent_id") and event["parent_id"] not in known):
            raise ValueError("invalid event IDs or parent ordering")
        if index >= start:
            policy = event["replay_policy"]
            if policy not in _POLICIES:
                raise ValueError(f"unsupported replay policy: {policy}")
            if policy == "BLOCKED":
                raise PermissionError(f"event {event['id']} is blocked")
            side_effect = event["type"] in {"tool.call", "human", "handoff", "state.change"}
            approval = policy == "REQUIRES_APPROVAL" or (side_effect and policy != "READ_ONLY")
            if approval and event["id"] not in approved:
                raise PermissionError(f"event {event['id']} requires approval")
            for binding in event["attributes"].get("input_bindings", {}).values():
                if binding.get("event_id") not in known:
                    raise ValueError("input bindings must reference a preceding event")
                path = binding.get("path", "")
                if not isinstance(path, str) or (path and not path.startswith("/")):
                    raise ValueError("output binding paths must be JSON pointers")
            callbacks.append(registry.resolve(event))
        known.add(event["id"])
    branch = copy.deepcopy(recording)
    branch["id"] = f"run_{uuid.uuid4()}"
    branch["events"] = branch["events"][:start]
    branch["status"], branch["ended_at"] = "running", None
    branch["started_at"] = _now()
    branch["metadata"]["lineage"] = {
        "original_run_id": recording["id"],
        "fork_event": from_event,
        "branch_id": branch["id"],
    }
    branch["metadata"]["rerun"] = {"from_event": from_event, "replace_models": replace_models or {}}
    for event in branch["events"]:
        event["run_id"] = branch["id"]
    for original, callback in zip(recording["events"][start:], callbacks, strict=True):
        event = copy.deepcopy(original)
        event["run_id"] = branch["id"]
        _bind(event, branch)
        if event["type"] == "generation":
            replacement = model or (replace_models or {}).get(event["attributes"].get("model"))
            if replacement:
                event["attributes"]["model"] = replacement
                if isinstance(event["input"], dict) and "model" in event["input"]:
                    event["input"]["model"] = replacement
        for metric in (
            "input_tokens",
            "output_tokens",
            "cache_read_tokens",
            "cache_write_tokens",
            "total_tokens",
            "cost_usd",
            "cost_is_estimate",
            "ttft_ms",
        ):
            event["attributes"].pop(metric, None)
        event["timestamp"] = _now()
        started = time.perf_counter()
        result = callback(copy.deepcopy(event), copy.deepcopy(branch))
        if inspect.isawaitable(result):
            result = await result
        if not isinstance(result, dict) or "output" not in result:
            raise ValueError("executors must return {'output': ..., 'attributes': {...}}")
        attributes = result.get("attributes") or {}
        if not isinstance(attributes, dict):
            raise ValueError("executor attributes must be an object")
        event["output"] = _snapshot(result["output"])
        event["attributes"].update(_snapshot(attributes))
        event["status"] = "completed"
        event["duration_ms"] = (time.perf_counter() - started) * 1000
        branch["events"].append(event)
    branch["status"], branch["ended_at"] = "completed", _now()
    return _snapshot(branch)


def rerun(recording: dict, registry: ExecutorRegistry, **kwargs) -> dict:
    """Synchronous entrypoint. Async applications should await rerun_async instead."""
    try:
        asyncio.get_running_loop()
    except RuntimeError:
        return asyncio.run(rerun_async(recording, registry, **kwargs))
    raise RuntimeError("use await rerun_async inside an async application")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("recording", nargs="?")
    parser.add_argument("--executor", required=True, help="Trusted Python module:function")
    parser.add_argument(
        "--stdio", action="store_true", help="Execute one JSON {event,context} request"
    )
    parser.add_argument("--from", dest="from_event")
    parser.add_argument("--model")
    parser.add_argument("--replace-model", action="append", default=[])
    parser.add_argument("--approve", action="append", default=[])
    parser.add_argument("--allow-live", action="store_true")
    parser.add_argument("-o", "--output")
    options = parser.parse_args()
    module_name, separator, function_name = options.executor.partition(":")
    if not separator:
        parser.error("--executor must be module:function")
    callback = getattr(importlib.import_module(module_name), function_name)
    if options.stdio:
        request = json.load(sys.stdin)
        result = callback(request["event"], request["context"])
        if inspect.isawaitable(result):

            async def await_result():
                return await result

            result = asyncio.run(await_result())
        print(json.dumps(result, allow_nan=False))
        return
    if not options.recording or not options.output or not options.from_event:
        parser.error("recording, --from and --output are required unless --stdio is used")
    registry = ExecutorRegistry()
    registry.register("*", callback)
    replacements = {}
    for value in options.replace_model:
        old, separator, new = value.partition("=")
        if not separator or not old or not new:
            parser.error("--replace-model must be old=new")
        replacements[old] = new
    branch = rerun(
        unpack(Path(options.recording).read_bytes()),
        registry,
        from_event=options.from_event,
        model=options.model,
        replace_models=replacements,
        approved_events=set(options.approve),
        allow_live=options.allow_live,
    )
    with Path(options.output).open("xb") as target:
        target.write(pack(branch))


if __name__ == "__main__":
    main()
