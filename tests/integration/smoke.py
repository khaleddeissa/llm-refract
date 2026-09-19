"""Exercise the running stack with only the Python standard library."""

import hashlib
import json
import os
import urllib.error
import urllib.request
import uuid
from pathlib import Path

base = os.environ.get("REFRACT_SERVER_URL", "http://127.0.0.1:8000")
root = Path(__file__).resolve().parents[2]


def request(path, body=None, expected=200):
    req = urllib.request.Request(
        base + path,
        None if body is None else json.dumps(body).encode(),
        {"Content-Type": "application/json"},
    )
    try:
        response = urllib.request.urlopen(req, timeout=10)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        data = response.read()
        assert response.status == expected, (response.status, data)
        return data


request("/v1/ready")
run = json.loads((root / "tests/fixtures/simple-run/execution.json").read_text())
run["id"] = f"smoke_{uuid.uuid4()}"
for event in run["events"]:
    event["run_id"] = run["id"]
run["metadata"]["api_key"] = "must-not-persist"
created = json.loads(request("/v1/runs", run, 201))
assert created["metadata"]["api_key"] == "[REDACTED]"
request("/v1/runs", run, 409)
prefix = f"/v1/runs/{run['id']}"
recorded = json.loads(request(prefix + "/replay", {"mode": "exact"}))
assert len(recorded["steps"]) == 2
request(prefix + "/replay", {"mode": "live"}, 400)
branch = json.loads(request(prefix + "/fork", {"from_event": "evt_2"}, 201))
assert len(branch["events"]) == 1
comparison = json.loads(request("/v1/diff", {"left": run["id"], "right": branch["id"]}))
assert comparison["first_divergence"] == 1
header, payload = request(prefix + "/artifact").split(b"\n", 1)
assert json.loads(header)["sha256"] == hashlib.sha256(payload).hexdigest()
assert b"must-not-persist" not in payload
assert json.loads(payload)["events"]
assert b"<title>Refract" in request("/")
print("Stack smoke test passed: ingest, redaction, conflict, playback, fork, diff, export, UI")
