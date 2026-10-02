"""Authenticated, transactional webhook inbox with duplicate and stale-version rejection."""

from __future__ import annotations

import hashlib
import hmac
import json
import os
import sqlite3
import threading
from pathlib import Path


def verify_delivery(body: bytes, signature: str, secret: str, delivery_id: str) -> dict:
    """Verify the original HTTP bytes before decoding. Never verify reserialized JSON."""
    if not secret or len(body) > 17 * 1024 * 1024:
        raise ValueError("missing webhook secret or oversized delivery")
    expected = "sha256=" + hmac.new(secret.encode(), body, hashlib.sha256).hexdigest()
    if not hmac.compare_digest(signature.encode(), expected.encode()):
        raise ValueError("invalid webhook signature")
    message = json.loads(body)
    if not isinstance(message, dict) or message.get("id") != delivery_id:
        raise ValueError("delivery id mismatch")
    for key in ("id", "run_id"):
        if not isinstance(message.get(key), str) or not 1 <= len(message[key]) <= 512:
            raise ValueError("invalid delivery identity")
    scope = message.get("scope")
    if not isinstance(scope, dict) or set(scope) != {"organization", "project", "environment"}:
        raise ValueError("invalid delivery scope")
    if any(not isinstance(v, str) or not 1 <= len(v) <= 128 for v in scope.values()):
        raise ValueError("invalid delivery scope")
    version = message.get("version")
    if not isinstance(version, int) or isinstance(version, bool) or not 0 <= version <= 2**53 - 1:
        raise ValueError("invalid delivery version")
    if message.get("operation") not in {"put", "delete"}:
        raise ValueError("invalid delivery operation")
    if message["operation"] == "put" and not isinstance(message.get("payload"), str):
        raise ValueError("put requires a stored payload")
    return message


class WebhookInbox:
    """Atomically persist receipts, latest payloads and deletion tombstones in SQLite.

    Return HTTP 2xx only after accept() returns. A retry after commit is a duplicate; delayed
    older versions cannot overwrite a newer value or resurrect a deleted execution. External
    side effects require their own transaction/idempotency contract, not an on-receive callback.
    """

    def __init__(self, path: str | Path, *, secret: str):
        if not secret:
            raise ValueError("webhook secret is required")
        self.secret = secret
        if str(path) != ":memory:":
            try:
                descriptor = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
                os.close(descriptor)
            except FileExistsError:
                pass
        self._db = sqlite3.connect(path, timeout=30, isolation_level=None, check_same_thread=False)
        self._lock = threading.RLock()
        self._db.executescript("""
            PRAGMA journal_mode=WAL;
            PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS receipts (id TEXT PRIMARY KEY, digest TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS deliveries (
                organization TEXT, project TEXT, environment TEXT, run_id TEXT,
                version INTEGER NOT NULL, payload TEXT, deleted INTEGER NOT NULL,
                PRIMARY KEY (organization,project,environment,run_id)
            );
        """)

    def accept(self, body: bytes, *, signature: str, delivery_id: str) -> str:
        message = verify_delivery(body, signature, self.secret, delivery_id)
        digest = hashlib.sha256(body).hexdigest()
        scope = message["scope"]
        identity = (
            scope["organization"],
            scope["project"],
            scope["environment"],
            message["run_id"],
        )
        with self._lock:
            self._db.execute("BEGIN IMMEDIATE")
            try:
                receipt = self._db.execute(
                    "SELECT digest FROM receipts WHERE id=?", (delivery_id,)
                ).fetchone()
                if receipt:
                    if receipt[0] != digest:
                        raise ValueError("delivery ID reused with different content")
                    self._db.execute("COMMIT")
                    return "duplicate"
                current = self._db.execute(
                    "SELECT version FROM deliveries WHERE organization=? AND project=? AND environment=? AND run_id=?",
                    identity,
                ).fetchone()
                result = "stale"
                if current is None or message["version"] > current[0]:
                    self._db.execute(
                        "INSERT INTO deliveries VALUES(?,?,?,?,?,?,?) ON CONFLICT(organization,project,environment,run_id) DO UPDATE SET version=excluded.version,payload=excluded.payload,deleted=excluded.deleted",
                        (
                            *identity,
                            message["version"],
                            message.get("payload") if message["operation"] == "put" else None,
                            int(message["operation"] == "delete"),
                        ),
                    )
                    result = "applied"
                self._db.execute("INSERT INTO receipts VALUES(?,?)", (delivery_id, digest))
                self._db.execute("COMMIT")
                return result
            except BaseException:
                self._db.execute("ROLLBACK")
                raise

    def get(self, run_id: str, *, organization: str, project: str, environment: str) -> str | None:
        with self._lock:
            row = self._db.execute(
                "SELECT payload FROM deliveries WHERE organization=? AND project=? AND environment=? AND run_id=? AND deleted=0",
                (organization, project, environment, run_id),
            ).fetchone()
            return row[0] if row else None

    def close(self) -> None:
        with self._lock:
            self._db.close()
