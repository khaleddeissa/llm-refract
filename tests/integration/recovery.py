"""Disposable local upgrade, encrypted backup/restore, load and process recovery rehearsal.

Never accepts an existing database. All writes stay in a new temporary directory.
Run after `cargo build -p refract-server`: python3 tests/integration/recovery.py.
"""

import base64
import concurrent.futures
import contextlib
import hashlib
import json
import os
import signal
import socket
import sqlite3
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
BINARY = ROOT / "target/debug/refract-server"
KEY = "local-recovery-fixture-key-never-use-in-production"
OLD = base64.b64encode(bytes([31]) * 32).decode()
NEW = base64.b64encode(bytes([32]) * 32).decode()


def request(url, path, body=None, method=None, expected=200):
    req = urllib.request.Request(
        url + path,
        data=None if body is None else json.dumps(body).encode(),
        headers={"Authorization": f"Bearer {KEY}", "Content-Type": "application/json"},
        method=method,
    )
    try:
        response = urllib.request.urlopen(req, timeout=10)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        payload = response.read()
        assert response.status == expected, (path, response.status, payload)
        return json.loads(payload) if payload else None


@contextlib.contextmanager
def service(database, keyring=False):
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    env = {key: value for key, value in os.environ.items() if not key.startswith("REFRACT_")}
    env.update(
        REFRACT_BIND=f"127.0.0.1:{port}",
        REFRACT_DATABASE_URL=f"sqlite://{database}",
        REFRACT_API_KEYS=json.dumps(
            [
                dict(
                    id="fixture-admin",
                    key=KEY,
                    role="admin",
                    organization="local",
                    project="default",
                    environment="development",
                )
            ]
        ),
        REFRACT_RATE_LIMIT="1000",
    )
    if keyring:
        path = database.parent / "keyring.json"
        path.write_text(json.dumps({"active": "next", "keys": {"legacy": OLD, "next": NEW}}))
        path.chmod(0o600)
        env["REFRACT_ENCRYPTION_KEYS_FILE"] = str(path)
    else:
        env["REFRACT_ENCRYPTION_KEY"] = OLD
    with tempfile.TemporaryFile() as log:
        process = subprocess.Popen([str(BINARY)], cwd=ROOT, env=env, stdout=log, stderr=log)
        try:
            url = f"http://127.0.0.1:{port}"
            deadline = time.monotonic() + 20
            while True:
                if process.poll() is not None:
                    log.seek(0)
                    raise AssertionError(log.read().decode())
                try:
                    request(url, "/v1/ready")
                    break
                except (OSError, AssertionError):
                    if time.monotonic() > deadline:
                        raise AssertionError("server readiness timeout") from None
                    time.sleep(0.05)
            yield url, process
        finally:
            if process.poll() is None:
                process.send_signal(signal.SIGTERM)
                try:
                    assert process.wait(timeout=10) == 0, "graceful shutdown failed"
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
                    raise AssertionError("shutdown timed out") from None


def seed_legacy(database):
    fixture = json.loads((ROOT / "tests/fixtures/simple-run/execution.json").read_text())
    migration = (ROOT / "crates/refract-storage/migrations/0001_initial.sql").read_bytes()
    with sqlite3.connect(database) as db:
        db.executescript(migration.decode())
        db.execute("""CREATE TABLE _sqlx_migrations (
            version BIGINT PRIMARY KEY, description TEXT, installed_on TEXT DEFAULT CURRENT_TIMESTAMP,
            success BOOLEAN, checksum BLOB, execution_time BIGINT)""")
        db.execute(
            "INSERT INTO _sqlx_migrations(version,description,success,checksum,execution_time) VALUES(1,'initial',1,?,0)",
            (hashlib.sha384(migration).digest(),),
        )
        db.execute(
            "INSERT INTO runs VALUES(?,?,?,?,?)",
            (
                fixture["id"],
                fixture["name"],
                fixture["status"],
                fixture["started_at"],
                json.dumps(fixture),
            ),
        )
    return fixture


def main():
    assert BINARY.is_file(), "build refract-server first"
    with tempfile.TemporaryDirectory(prefix="refract-recovery-") as directory:
        root = Path(directory)
        database, backup, restored = (
            root / name for name in ("active.db", "backup.db", "restored.db")
        )
        fixture = seed_legacy(database)
        with service(database) as (url, _):
            assert request(url, f"/v1/runs/{fixture['id']}")["id"] == fixture["id"]

            def ingest(index):
                run = json.loads(json.dumps(fixture))
                run["id"] = f"recovery-{index}"
                for event in run["events"]:
                    event["run_id"] = run["id"]
                request(url, "/v1/runs", run, expected=201)

            with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
                list(pool.map(ingest, range(32)))
            request(
                url,
                f"/v1/runs/{fixture['id']}/embedding",
                {"model": "fixture", "values": [1, 0]},
                method="PUT",
            )
            # SQLite's online backup API includes committed WAL pages; never copy the live file alone.
            with sqlite3.connect(database) as source, sqlite3.connect(backup) as destination:
                source.backup(destination)
                assert destination.execute("SELECT COUNT(*) FROM runs").fetchone()[0] == 33
                assert (
                    destination.execute("SELECT COUNT(*) FROM _sqlx_migrations").fetchone()[0] == 5
                )
        with sqlite3.connect(backup) as source, sqlite3.connect(restored) as destination:
            source.backup(destination)
        with service(restored, keyring=True) as (url, process):
            request(url, "/v1/admin/encryption/rotate", {"limit": 100})
            assert request(url, "/v1/runs/recovery-31")["id"] == "recovery-31"
            result = request(
                url,
                "/v1/search/vector",
                {"embedding": {"model": "fixture", "values": [1, 0]}, "limit": 1},
            )
            assert fixture["id"] in json.dumps(result)
            with sqlite3.connect(restored) as db:
                assert (
                    db.execute(
                        "SELECT COUNT(*) FROM runs WHERE execution LIKE 'enc:v2:next:%'"
                    ).fetchone()[0]
                    == 33
                )
            # Crash the disposable process after acknowledged commits, then reopen the same database.
            process.kill()
            process.wait(timeout=10)
        with service(restored, keyring=True) as (url, _):
            assert request(url, "/v1/runs/recovery-31")["id"] == "recovery-31"
    print(
        "Recovery passed: v1→v5 migration, 32 concurrent writes, encrypted online backup/restore, key rotation, SIGTERM and crash restart"
    )


if __name__ == "__main__":
    main()
