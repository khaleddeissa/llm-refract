"""Upload native canonical JSON snapshots through the Rust validation/storage boundary."""

import argparse
import json
import urllib.error
import urllib.request
from pathlib import Path


def ingest(path: Path, endpoint: str) -> str:
    if path.stat().st_size > 16 * 1024 * 1024:
        raise ValueError("snapshot exceeds 16 MiB")
    body = path.read_bytes()
    json.loads(body)
    request = urllib.request.Request(
        endpoint.rstrip("/") + "/v1/runs", body, {"Content-Type": "application/json"}, method="POST"
    )
    try:
        with urllib.request.urlopen(request, timeout=15) as response:
            return json.load(response)["id"]
    except urllib.error.HTTPError as error:
        raise RuntimeError(f"Ingestion failed: HTTP {error.code}") from error


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("files", type=Path, nargs="+")
    parser.add_argument("--endpoint", default="http://localhost:8000")
    args = parser.parse_args()
    for path in args.files:
        print(ingest(path, args.endpoint))


if __name__ == "__main__":
    main()
