import copy

import pytest

import refract
from refract.rerun import ExecutorRegistry, rerun


def recording():
    with refract.run("experiment") as run:
        first = refract.event(
            type="retrieval", name="search", input={"query": "policy"}, output={"text": "30 days"}
        )
        refract.event(
            type="generation",
            name="answer",
            input={"context": "30 days", "model": "old"},
            output="old answer",
            parent_id=first,
            attributes={
                "model": "old",
                "input_tokens": 123,
                "input_bindings": {"context": {"event_id": first, "path": "/text"}},
            },
        )
    return run.snapshot()


def test_rerun_binds_fresh_outputs_changes_model_and_preserves_source():
    source = recording()
    pristine = copy.deepcopy(source)
    registry = ExecutorRegistry()
    registry.register("search", lambda event, context: {"output": {"text": "14 days"}})

    def generate(event, context):
        assert event["input"]["model"] == "new"
        assert event["input"]["context"] == "14 days"
        assert "input_tokens" not in event["attributes"]
        assert context["events"][0]["output"]["text"] == "14 days"
        return {"output": "Returns within 14 days", "attributes": {"input_tokens": 10}}

    registry.register("generation", generate)
    branch = rerun(
        source,
        registry,
        from_event=source["events"][0]["id"],
        replace_models={"old": "new"},
        allow_live=True,
    )
    assert branch["id"] != source["id"]
    assert branch["events"][1]["attributes"]["input_tokens"] == 10
    assert branch["metadata"]["lineage"]["original_run_id"] == source["id"]
    assert source == pristine


def test_preflight_blocks_all_execution_and_approvals_allow_explicit_tools():
    source = recording()
    source["events"][1]["type"] = "tool.call"
    calls = []
    registry = ExecutorRegistry()
    registry.register(
        "*", lambda event, context: calls.append(event["id"]) or {"output": {"text": "new"}}
    )
    options = {"from_event": source["events"][0]["id"], "allow_live": True}
    with pytest.raises(PermissionError, match="requires approval"):
        rerun(source, registry, **options)
    assert calls == []
    result = rerun(source, registry, **options, approved_events={source["events"][1]["id"]})
    assert len(result["events"]) == 2
    calls.clear()
    source["events"][1]["replay_policy"] = "BLOCKED"
    with pytest.raises(PermissionError, match="blocked"):
        rerun(source, registry, **options, approved_events={source["events"][1]["id"]})
    assert calls == []


def test_missing_executor_and_live_opt_in_are_explicit():
    source = recording()
    registry = ExecutorRegistry()
    with pytest.raises(PermissionError, match="allow_live"):
        rerun(source, registry, from_event=source["events"][0]["id"])
    with pytest.raises(ValueError, match="no executor"):
        rerun(source, registry, from_event=source["events"][0]["id"], allow_live=True)
