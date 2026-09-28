import asyncio
from typing import TypedDict

import pytest

import refract
from refract.integrations.langgraph import LangGraphRuntime


def graph_fixture():
    pytest.importorskip("langgraph")
    from langgraph.checkpoint.memory import InMemorySaver
    from langgraph.graph import END, START, StateGraph

    class State(TypedDict):
        value: int

    graph = StateGraph(State)
    graph.add_node("double", lambda state: {"value": state["value"] * 2})
    graph.add_node("increment", lambda state: {"value": state["value"] + 1})
    graph.add_edge(START, "double")
    graph.add_edge("double", "increment")
    graph.add_edge("increment", END)
    return graph.compile(checkpointer=InMemorySaver(), interrupt_before=["increment"])


@pytest.mark.parametrize("asynchronous", [False, True])
def test_resume_real_langgraph_checkpoint_branches_recorded_state(asynchronous):
    graph = graph_fixture()
    runtime = LangGraphRuntime(graph, name="local-counter")
    config = {"configurable": {"thread_id": "authorized-thread"}}
    assert graph.invoke({"value": 2}, config)["value"] == 4
    with refract.run("original"):
        checkpoint = runtime.checkpoint(config)
    assert checkpoint["output"]["value"] == 4
    with pytest.raises(PermissionError):
        runtime.resume(checkpoint, thread_id="other-thread", allow_live=True)
    with pytest.raises(PermissionError):
        runtime.resume(checkpoint, thread_id="authorized-thread")
    kwargs = dict(thread_id="authorized-thread", allow_live=True, updates={"value": 10})
    branch = (
        asyncio.run(runtime.resume_async(checkpoint, **kwargs))
        if asynchronous
        else runtime.resume(checkpoint, **kwargs)
    )
    result = next(event for event in branch["events"] if event["name"] == "continuation.result")
    assert result["output"]["value"] == 11
    assert branch["metadata"]["lineage"]["fork_event"] == checkpoint["id"]
    assert checkpoint["output"]["value"] == 4
    assert any(event["attributes"].get("framework") == "langchain" for event in branch["events"])
