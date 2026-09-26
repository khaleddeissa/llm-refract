import sys
from types import ModuleType

import refract
from refract.integrations.langchain import langchain_handler


def test_langchain_records_nested_actual_tool_execution_and_usage(monkeypatch):
    module = ModuleType("langchain_core.callbacks.base")
    module.BaseCallbackHandler = type("BaseCallbackHandler", (), {})
    monkeypatch.setitem(sys.modules, "langchain_core.callbacks.base", module)
    with refract.run("chain") as run:
        handler = langchain_handler(provider="custom")
        handler.on_chain_start({"name": "agent"}, {"question": "hi"}, run_id="chain")
        handler.on_tool_start({"name": "search"}, "hi", run_id="tool", parent_run_id="chain")
        handler.on_tool_end({"documents": ["answer"]}, run_id="tool")
        handler.on_llm_start({"name": "answer"}, ["hi"], run_id="model", parent_run_id="chain")
        handler.on_llm_new_token("hello", run_id="model")
        handler.on_llm_end(
            {"llm_output": {"token_usage": {"prompt_tokens": 10, "completion_tokens": 2}}},
            run_id="model",
        )
        handler.on_chain_end("hello", run_id="chain")
    events = run.data["events"]
    assert events[1]["parent_id"] == events[0]["id"]
    assert events[1]["output"] == {"documents": ["answer"]}
    assert events[1]["replay_policy"] == "REQUIRES_APPROVAL"
    assert events[2]["attributes"]["input_tokens"] == 10
    assert events[2]["attributes"]["ttft_ms"] >= 0
    assert handler.errors == []


def test_installed_langchain_runnable_emits_actual_callbacks():
    import pytest

    runnables = pytest.importorskip("langchain_core.runnables")
    with refract.run("langchain") as recording:
        handler = langchain_handler()
        chain = runnables.RunnableLambda(lambda value: {"answer": value["question"].upper()})
        assert chain.invoke({"question": "hello"}, config={"callbacks": [handler]}) == {
            "answer": "HELLO"
        }
    assert recording.data["events"][0]["output"] == {"answer": "HELLO"}
    assert handler.errors == []


def test_real_chat_model_usage_and_multiple_callbacks_coexist():
    import pytest

    pytest.importorskip("langchain_core")
    from langchain_core.callbacks import BaseCallbackHandler
    from langchain_core.language_models.fake_chat_models import FakeMessagesListChatModel
    from langchain_core.messages import AIMessage

    class OtherObserver(BaseCallbackHandler):
        completed = 0

        def on_llm_end(self, response, **kwargs):
            self.completed += 1

    observer = OtherObserver()
    model = FakeMessagesListChatModel(
        responses=[
            AIMessage(
                content="Hello",
                usage_metadata={"input_tokens": 2, "output_tokens": 1, "total_tokens": 3},
                tool_calls=[{"name": "lookup", "args": {"id": 1}, "id": "call-1"}],
                response_metadata={"api_key": "must-not-be-recorded"},
            )
        ]
    )
    with refract.run("chat") as recording:
        handler = langchain_handler(provider="private")
        result = model.invoke("Hi", config={"callbacks": [handler, observer]})
    assert result.content == "Hello"
    assert observer.completed == 1
    assert recording.data["events"][0]["attributes"]["input_tokens"] == 2
    assert recording.data["events"][0]["attributes"]["output_tokens"] == 1
    message = recording.data["events"][0]["output"]["generations"][0][0]["message"]
    assert message["tool_calls"][0]["name"] == "lookup"
    assert message["usage_metadata"]["total_tokens"] == 3
    assert message["response_metadata"]["api_key"] == "[REDACTED]"
    assert handler.errors == []
