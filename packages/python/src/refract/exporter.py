"""Bounded background ingestion with sampling and an optional private retry spool."""

from __future__ import annotations

import hashlib
import json
import os
import queue
import threading
import time
import urllib.request
import uuid
from pathlib import Path
from typing import Any, Self

from . import _snapshot


class BackgroundExporter:
    """Submit snapshots without network/disk I/O on the application's thread.

    The worker spools before transmitting. A process crash before that worker write can
    lose queued events; this is a retry spool, not a synchronous write-ahead guarantee.
    Use one exporter per spool directory and call close during graceful shutdown.
    """

    def __init__(
        self,
        endpoint: str,
        *,
        api_key: str | None = None,
        project: str | None = None,
        queue_size: int = 256,
        batch_size: int = 32,
        sample_rate: float = 1.0,
        flush_interval: float = 1.0,
        timeout: float = 5.0,
        retries: int = 2,
        spool_dir: str | Path | None = None,
        max_spool_bytes: int = 64 * 1024 * 1024,
        max_queue_bytes: int = 16 * 1024 * 1024,
    ):
        if not endpoint.startswith(("http://", "https://")):
            raise ValueError("endpoint must be an HTTP(S) URL")
        if not 0 <= sample_rate <= 1 or not 1 <= batch_size <= 100 or queue_size < 1:
            raise ValueError("invalid sampling rate, batch size (1..100), or queue size")
        if min(flush_interval, timeout, max_queue_bytes, max_spool_bytes) <= 0 or retries < 0:
            raise ValueError("timeouts and capacity must be positive; retries must be nonnegative")
        self.endpoint = endpoint.rstrip("/") + "/v1/runs/batch"
        self.headers = {"Content-Type": "application/json"}
        if api_key:
            self.headers["Authorization"] = f"Bearer {api_key}"
        if project:
            self.headers["X-Refract-Project"] = project
        self.batch_size, self.sample_rate = batch_size, sample_rate
        self.flush_interval, self.timeout, self.retries = flush_interval, timeout, retries
        self.spool_dir = Path(spool_dir) if spool_dir is not None else None
        self.max_spool_bytes, self.max_queue_bytes = max_spool_bytes, max_queue_bytes
        self._queue: queue.Queue[bytes] = queue.Queue(queue_size)
        self._lock = threading.Lock()
        self._stop = threading.Event()
        self._wake = threading.Event()
        self._closed = False
        self._queued_bytes = 0
        self.last_error: str | None = None
        self.stats = dict(accepted=0, sent=0, sampled_out=0, dropped=0, failed=0, recovered=0)
        if self.spool_dir:
            self.spool_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
        self._worker = threading.Thread(target=self._work, name="refract-export", daemon=True)
        self._worker.start()

    def submit(self, run: dict[str, Any]) -> bool:
        """Return False when sampled out, closed, malformed, or over capacity; never raise."""
        try:
            identity = str(run["id"])
            fraction = int.from_bytes(hashlib.sha256(identity.encode()).digest()[:8], "big") / 2**64
            if fraction >= self.sample_rate:
                self._count("sampled_out")
                return False
            body = json.dumps(_snapshot(run), allow_nan=False, separators=(",", ":")).encode()
            with self._lock:
                if self._closed or self._queued_bytes + len(body) > self.max_queue_bytes:
                    self.stats["dropped"] += 1
                    return False
                try:
                    self._queue.put_nowait(body)
                except queue.Full:
                    self.stats["dropped"] += 1
                    return False
                self._queued_bytes += len(body)
                self.stats["accepted"] += 1
            if self._queue.qsize() >= self.batch_size:
                self._wake.set()
            return True
        except Exception as error:
            self.last_error = type(error).__name__
            self._count("dropped")
            return False

    def _count(self, name: str, amount: int = 1) -> None:
        with self._lock:
            self.stats[name] += amount

    def _spool(self, body: bytes) -> Path | None:
        if self.spool_dir is None:
            return None
        size = sum(p.stat().st_size for p in self.spool_dir.glob("*.json") if not p.is_symlink())
        if size + len(body) > self.max_spool_bytes:
            raise OSError("retry spool capacity exceeded")
        target = self.spool_dir / f"{uuid.uuid4().hex}.json"
        temporary = target.with_suffix(".tmp")
        descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(body)
            stream.flush()
            os.fsync(stream.fileno())
        temporary.replace(target)
        return target

    def _transmit(self, entries: list[tuple[bytes, Path | None]]) -> bool:
        body = b'{"runs":[' + b",".join(item[0] for item in entries) + b"]}"
        for attempt in range(self.retries + 1):
            try:
                request = urllib.request.Request(self.endpoint, body, self.headers, method="POST")
                with urllib.request.urlopen(request, timeout=self.timeout) as response:
                    response.read()
                for _, path in entries:
                    if path:
                        path.unlink(missing_ok=True)
                self._count("sent", len(entries))
                self.last_error = None
                return True
            except Exception as error:
                self.last_error = type(error).__name__
                if attempt < self.retries:
                    self._stop.wait(min(0.1 * 2**attempt, 2))
        self._count("failed", len(entries))
        return False

    def _recover(self) -> None:
        if self.spool_dir is None:
            return
        entries: list[tuple[bytes, Path | None]] = []
        for path in self.spool_dir.glob("*.json"):
            if path.is_symlink() or path.stat().st_size > self.max_spool_bytes:
                continue
            try:
                body = path.read_bytes()
                run = json.loads(body)
                if not isinstance(run, dict) or run.get("spec_version") != "refract.execution.v1":
                    raise ValueError("invalid retry spool entry")
                entries.append((body, path))
            except (OSError, ValueError):
                self.last_error = "InvalidSpoolEntry"
                continue
            if len(entries) == self.batch_size:
                break
        if entries and self._transmit(entries):
            self._count("recovered", len(entries))

    def _work(self) -> None:
        while True:
            self._wake.wait(self.flush_interval)
            self._wake.clear()
            try:
                self._recover()
                pending: list[bytes] = []
                for _ in range(self.batch_size):
                    try:
                        pending.append(self._queue.get_nowait())
                    except queue.Empty:
                        break
                if pending:
                    entries: list[tuple[bytes, Path | None]] = []
                    try:
                        for body in pending:
                            try:
                                entries.append((body, self._spool(body)))
                            except OSError as error:
                                self.last_error = type(error).__name__
                                entries.append((body, None))
                        self._transmit(entries)
                    finally:
                        for body in pending:
                            with self._lock:
                                self._queued_bytes -= len(body)
                            self._queue.task_done()
            except Exception as error:
                self.last_error = type(error).__name__
            if not self._queue.empty():
                self._wake.set()
            if self._stop.is_set() and self._queue.empty():
                return

    def flush(self, timeout: float = 10.0) -> bool:
        """Wait up to timeout for delivery, returning False if retry files remain."""
        self._wake.set()
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            spool_pending = self.spool_dir and any(self.spool_dir.glob("*.json"))
            if self._queue.unfinished_tasks == 0 and not spool_pending:
                return self.last_error is None
            time.sleep(0.01)
        return False

    def close(self, timeout: float = 10.0) -> bool:
        with self._lock:
            self._closed = True
        self._stop.set()
        self._wake.set()
        self._worker.join(timeout)
        return not self._worker.is_alive() and self.last_error is None

    def __enter__(self) -> Self:
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.close()
