"""Measure deterministic artifact serialization for the shared fixture."""

import argparse
import json
import time
from pathlib import Path

from refract.artifact import pack

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--iterations", type=int, default=1000)
args = parser.parse_args()
if args.iterations < 1:
    parser.error("iterations must be positive")
root = Path(__file__).resolve().parents[2]
run = json.loads((root / "tests/fixtures/simple-run/execution.json").read_text())
started = time.perf_counter()
for _ in range(args.iterations):
    artifact = pack(run)
elapsed = time.perf_counter() - started
print(
    json.dumps(
        {
            "iterations": args.iterations,
            "seconds": elapsed,
            "artifacts_per_second": args.iterations / elapsed,
            "artifact_bytes": len(artifact),
        }
    )
)
