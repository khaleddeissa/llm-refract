import refract
from refract.integrations import manual


def test_provider_neutral_generation():
    with refract.run("adapter") as run:
        result = manual.generation(
            lambda: {"answer": 42}, provider="local", model="demo", prompt="test"
        )
    event = run.snapshot()["events"][0]
    assert result == event["output"] == {"answer": 42}
    assert event["attributes"]["provider"] == "local"
    assert event["duration_ms"] >= 0
