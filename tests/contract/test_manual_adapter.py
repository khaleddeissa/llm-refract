import importlib.util
from pathlib import Path

import refract

spec = importlib.util.spec_from_file_location(
    "manual", Path(__file__).parents[2] / "integrations/manual.py"
)
manual = importlib.util.module_from_spec(spec)
spec.loader.exec_module(manual)


def test_provider_neutral_generation():
    with refract.run("adapter") as run:
        result = manual.generation(
            lambda: {"answer": 42}, provider="local", model="demo", prompt="test"
        )
    event = run.snapshot()["events"][0]
    assert result == event["output"] == {"answer": 42}
    assert event["attributes"]["provider"] == "local"
    assert event["duration_ms"] >= 0
