import asyncio
import json

import pytest

import refract
from refract.artifact import unpack


def test_recording_redacts_and_snapshots(tmp_path):
    path = tmp_path / "recording.rfr"
    original = {"email": "private@example.com", "count": 1}
    with refract.run("demo", path=path) as run:
        refract.event(type="generation", name="answer", input=original, output="hello")
        original["count"] = 99
    assert run.data["status"] == "completed"
    event = unpack(path.read_bytes())["events"][0]
    assert event["input"] == {"email": "[REDACTED]", "count": 1}
    with pytest.raises(FileExistsError):
        run.export(path)


def test_failure_resets_context():
    with pytest.raises(ValueError), refract.run("failed") as run:
        raise ValueError("secret message")
    assert run.data["status"] == "failed"
    assert "secret message" not in json.dumps(run.data)
    with pytest.raises(RuntimeError):
        refract.event(type="error", name="outside")


def test_async_context_isolation():
    async def worker(name):
        with refract.run(name) as run:
            await asyncio.sleep(0)
            refract.event(type="tool.call", name=name)
        return run.snapshot()

    async def main():
        return await asyncio.gather(worker("a"), worker("b"))

    runs = asyncio.run(main())
    assert [r["events"][0]["name"] for r in runs] == ["a", "b"]


def test_decorator_awaits_function():
    @refract.trace
    async def agent():
        await asyncio.sleep(0)
        refract.event(type="decision", name="inside")
        return 42

    assert asyncio.run(agent()) == 42
