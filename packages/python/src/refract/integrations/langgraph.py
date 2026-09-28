"""Capture and continue checkpoints in an application-owned LangGraph runtime."""

from __future__ import annotations

import copy
from typing import Any

from refract import _current, run
from refract.instrumentation import _json
from refract.integrations.langchain import langchain_handler


class LangGraphRuntime:
    """Bind recordings to one trusted compiled graph and its configured checkpointer.

    No graph code, imports, credentials or checkpoint store are loaded from recordings.
    Continuation must name the authorized thread independently of the recorded reference.
    """

    def __init__(self, graph, *, name: str):
        if not name.strip():
            raise ValueError("graph name is required")
        self.graph, self.name = graph, name

    def checkpoint(self, config: dict) -> dict:
        active = _current.get()
        if active is None:
            raise RuntimeError("capture checkpoints inside refract.run")
        state = self.graph.get_state(config)
        reference = state.config.get("configurable", {})
        if not reference.get("checkpoint_id") or not reference.get("thread_id"):
            raise ValueError("graph must have a saved checkpoint")
        reference = {
            key: reference.get(key, "") for key in ("thread_id", "checkpoint_ns", "checkpoint_id")
        }
        active.event(
            type="checkpoint",
            name=self.name,
            output=_json(state.values),
            attributes={"framework": "langgraph", "graph": self.name, "checkpoint": reference},
            replay_policy="REQUIRES_APPROVAL",
        )
        return copy.deepcopy(active.data["events"][-1])

    def _config(self, checkpoint: dict, thread_id: str, allow_live: bool) -> dict:
        if not allow_live:
            raise PermissionError("checkpoint continuation requires allow_live=True")
        attrs = checkpoint.get("attributes", {})
        if (
            checkpoint.get("type") != "checkpoint"
            or attrs.get("framework") != "langgraph"
            or attrs.get("graph") != self.name
        ):
            raise ValueError("checkpoint belongs to a different runtime")
        reference = attrs.get("checkpoint", {})
        if reference.get("thread_id") != thread_id:
            raise PermissionError("checkpoint thread does not match the authorized thread")
        if (
            not all(
                isinstance(reference.get(key), str)
                for key in ("thread_id", "checkpoint_ns", "checkpoint_id")
            )
            or not reference["checkpoint_id"]
        ):
            raise ValueError("invalid checkpoint reference")
        return {
            "configurable": {
                key: reference[key] for key in ("thread_id", "checkpoint_ns", "checkpoint_id")
            }
        }

    def resume(
        self, checkpoint: dict, *, thread_id: str, allow_live=False, updates=None, resume_value=None
    ) -> dict:
        """Fork from a saved checkpoint and record actual subsequent graph callbacks.

        `updates` branches state through LangGraph's reducers; `resume_value` answers a
        framework interrupt. Side effects use the application's configured graph policies.
        """
        config = self._config(checkpoint, thread_id, allow_live)
        state = self.graph.get_state(config)
        if (
            not state.config
            or state.config.get("configurable", {}).get("checkpoint_id")
            != config["configurable"]["checkpoint_id"]
            or not state.created_at
        ):
            raise ValueError("checkpoint is unavailable in this runtime's store")
        with run(
            f"{self.name}.continuation",
            metadata={
                "lineage": {
                    "original_run_id": checkpoint["run_id"],
                    "fork_event": checkpoint["id"],
                }
            },
        ) as recording:
            if updates is not None:
                config = self.graph.update_state(config, copy.deepcopy(updates))
            config = {**config, "callbacks": [langchain_handler()]}
            value: Any = None
            if resume_value is not None:
                from langgraph.types import Command

                value = Command(resume=resume_value)
            output = self.graph.invoke(value, config)
            recording.event(type="state.change", name="continuation.result", output=_json(output))
        return recording.snapshot()

    async def resume_async(
        self, checkpoint: dict, *, thread_id: str, allow_live=False, updates=None, resume_value=None
    ) -> dict:
        config = self._config(checkpoint, thread_id, allow_live)
        state = await self.graph.aget_state(config)
        if (
            not state.config
            or state.config.get("configurable", {}).get("checkpoint_id")
            != config["configurable"]["checkpoint_id"]
            or not state.created_at
        ):
            raise ValueError("checkpoint is unavailable in this runtime's store")
        with run(
            f"{self.name}.continuation",
            metadata={
                "lineage": {
                    "original_run_id": checkpoint["run_id"],
                    "fork_event": checkpoint["id"],
                }
            },
        ) as recording:
            if updates is not None:
                config = await self.graph.aupdate_state(config, copy.deepcopy(updates))
            config = {**config, "callbacks": [langchain_handler()]}
            value: Any = None
            if resume_value is not None:
                from langgraph.types import Command

                value = Command(resume=resume_value)
            output = await self.graph.ainvoke(value, config)
            recording.event(type="state.change", name="continuation.result", output=_json(output))
        return recording.snapshot()
