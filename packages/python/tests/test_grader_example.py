import importlib.util
import json
from pathlib import Path
from types import SimpleNamespace

import pytest


def load_example():
    filename = Path(__file__).resolve().parents[3] / "examples/python/evaluation_grader.py"
    spec = importlib.util.spec_from_file_location("evaluation_grader", filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_grader_uses_structured_output_and_threshold_without_live_network():
    calls = []

    def create(**kwargs):
        calls.append(kwargs)
        return SimpleNamespace(
            status="completed",
            output_text=json.dumps(
                {"score": 0.94, "reason": "Both promise a five business day refund."}
            ),
        )

    client = SimpleNamespace(responses=SimpleNamespace(create=create))
    result = load_example().grade(
        {"left": "5 business days", "right": "five working days", "threshold": 0.9},
        client=client,
        model="fixture-model",
    )
    assert result["equivalent"] is True
    assert result["grader"] == "openai/fixture-model"
    assert calls[0]["text"]["format"]["strict"] is True
    assert "UNTRUSTED EVIDENCE" in calls[0]["instructions"]
    assert calls[0]["store"] is False


@pytest.mark.parametrize("score", [True, float("nan"), -0.1, 2])
def test_grader_rejects_invalid_scores(score):
    response = SimpleNamespace(
        status="completed", output_text=json.dumps({"score": score, "reason": "invalid"})
    )
    client = SimpleNamespace(responses=SimpleNamespace(create=lambda **kwargs: response))
    with pytest.raises(ValueError):
        load_example().grade({"left": 1, "right": 2}, client=client, model="fixture-model")
