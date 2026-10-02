import json
import threading
import urllib.request

import refract
from refract.exporter import BackgroundExporter


class Response:
    def __enter__(self):
        return self

    def __exit__(self, *args):
        pass

    def read(self, size=-1):
        return b"{}"


def snapshot():
    with refract.run("export") as recording:
        refract.event(type="generation", name="hello", attributes={"input_tokens": 3})
    return recording.snapshot()


def test_exporter_batches_auth_sampling_and_graceful_close(monkeypatch):
    requests = []

    def send(request, **kwargs):
        requests.append(request)
        return Response()

    monkeypatch.setattr(
        urllib.request.OpenerDirector, "open", lambda self, *a, **kw: send(*a, **kw)
    )
    exporter = BackgroundExporter(
        "http://collector", api_key="private", batch_size=3, flush_interval=60
    )
    for _ in range(3):
        assert exporter.submit(snapshot())
    assert exporter.flush()
    assert exporter.close()
    assert len(requests) == 1
    assert requests[0].full_url == "http://collector/v1/runs/batch"
    assert requests[0].headers["Authorization"] == "Bearer private"
    assert len(json.loads(requests[0].data)["runs"]) == 3
    assert exporter.stats["sent"] == 3
    assert exporter.submit(snapshot()) is False
    with BackgroundExporter("http://collector", sample_rate=0) as sampled:
        assert sampled.submit(snapshot()) is False
        assert sampled.stats["sampled_out"] == 1


def test_retry_spool_recovers_after_restart(monkeypatch, tmp_path):
    def offline(*args, **kwargs):
        raise OSError("network down")

    monkeypatch.setattr(
        urllib.request.OpenerDirector, "open", lambda self, *a, **kw: offline(*a, **kw)
    )
    first = BackgroundExporter("http://collector", spool_dir=tmp_path, retries=0, flush_interval=60)
    assert first.submit(snapshot())
    assert first.close() is False
    files = list(tmp_path.glob("*.json"))
    assert len(files) == 1
    assert files[0].stat().st_mode & 0o777 == 0o600
    sent = []

    def online(request, **kwargs):
        sent.extend(json.loads(request.data)["runs"])
        return Response()

    monkeypatch.setattr(
        urllib.request.OpenerDirector, "open", lambda self, *a, **kw: online(*a, **kw)
    )
    second = BackgroundExporter(
        "http://collector", spool_dir=tmp_path, retries=0, flush_interval=60
    )
    assert second.flush()
    assert second.close()
    assert len(sent) == 1
    assert second.stats["recovered"] == 1
    assert list(tmp_path.glob("*.json")) == []


def test_bounded_queue_does_not_wait_for_network(monkeypatch):
    started = threading.Event()
    release = threading.Event()

    def slow(request, **kwargs):
        started.set()
        release.wait(2)
        return Response()

    monkeypatch.setattr(
        urllib.request.OpenerDirector, "open", lambda self, *a, **kw: slow(*a, **kw)
    )
    exporter = BackgroundExporter("http://collector", queue_size=1, batch_size=1)
    try:
        assert exporter.submit(snapshot())
        assert started.wait(1)
        assert exporter.submit(snapshot())
        assert exporter.submit(snapshot()) is False
        assert exporter.stats["dropped"] == 1
    finally:
        release.set()
        assert exporter.close()


def test_sdk_fail_open_and_strict_modes(monkeypatch):
    def offline(*args, **kwargs):
        raise OSError("network unavailable")

    monkeypatch.setattr(urllib.request, "urlopen", offline)
    with refract.run("safe", endpoint="http://collector") as recording:
        refract.event(type="decision", name="result", output=42)
    assert recording.recording_errors == ["OSError"]
    import pytest

    with (
        pytest.raises(OSError),
        refract.run("strict", endpoint="http://collector", fail_open=False),
    ):
        pass
    with (
        pytest.raises(ValueError, match="application"),
        refract.run("app", endpoint="http://collector"),
    ):
        raise ValueError("application")


def test_durable_acceptance_survives_worker_not_running(tmp_path, monkeypatch):
    import json
    import os
    import threading

    from refract import run
    from refract.exporter import BackgroundExporter

    monkeypatch.setattr(threading.Thread, "start", lambda self: None)
    fsync_calls = []
    original = os.fsync

    def synced(fd):
        fsync_calls.append(fd)
        original(fd)

    monkeypatch.setattr(os, "fsync", synced)
    with run("durable-offline") as recording:
        pass
    exporter = BackgroundExporter("http://localhost:1", spool_dir=tmp_path, durable=True)
    assert exporter.submit(recording.snapshot())
    paths = list(tmp_path.glob("*.json"))
    assert len(paths) == 1
    assert json.loads(paths[0].read_text())["id"] == recording.data["id"]
    assert len(fsync_calls) == 2  # file contents and directory rename
    assert exporter._queue.empty()
    full = BackgroundExporter(
        "http://localhost:1", spool_dir=tmp_path, durable=True, max_spool_bytes=1
    )
    assert not full.submit(recording.snapshot())
    assert full.stats["accepted"] == 0


def test_spool_defaults_to_fsync_acceptance_and_rejects_redirects(tmp_path, monkeypatch):
    monkeypatch.setattr(threading.Thread, "start", lambda self: None)
    exporter = BackgroundExporter("http://collector", spool_dir=tmp_path)
    assert exporter.durable is True
    assert exporter.submit(snapshot())
    assert len(list(tmp_path.glob("*.json"))) == 1
    assert any(type(handler).__name__ == "_NoRedirect" for handler in exporter._opener.handlers)
