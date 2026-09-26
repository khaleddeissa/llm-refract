"""Exchange Python, Node and Rust text artifacts; exercise legacy conversion."""

import json
import subprocess
import tempfile
from pathlib import Path

from refract.artifact import pack, unpack
from refract.otel import from_otlp, to_otlp

root = Path(__file__).resolve().parents[2]
cli = root / "target/debug/refract"
run = json.loads((root / "tests/fixtures/simple-run/execution.json").read_text())
with tempfile.TemporaryDirectory() as directory:
    directory = Path(directory)
    python_artifact = directory / "python.rfr"
    python_artifact.write_bytes(pack(run))
    subprocess.run([cli, "validate", python_artifact], check=True)
    rust_artifact = directory / "rust.rfr"
    subprocess.run([cli, "pack", python_artifact, "-o", rust_artifact], check=True)
    assert unpack(rust_artifact.read_bytes()) == run
    subprocess.run(["node", root / "tests/contract/artifact.mjs", rust_artifact], check=True)
    node_artifact = directory / "node.rfr"
    subprocess.run(
        ["node", root / "tests/contract/write.mjs", python_artifact, node_artifact], check=True
    )
    subprocess.run([cli, "validate", node_artifact], check=True)
    assert unpack(node_artifact.read_bytes()) == run
    # Usage and secrets must retain identical meaning through OTel and both SDKs.
    run["events"][1]["attributes"].update(
        {
            "input_tokens": 40,
            "output_tokens": 12,
            "total_tokens": 52,
            "cost_usd": 0.002,
            "api_key": "synthetic-secret",
        }
    )
    otlp = directory / "python-otlp.json"
    otlp.write_text(json.dumps(to_otlp(run)))
    returned = directory / "node-otlp.json"
    subprocess.run(
        ["node", root / "tests/contract/observability.mjs", otlp, node_artifact, returned],
        check=True,
    )
    subprocess.run([cli, "validate", node_artifact], check=True)
    metrics = json.loads(subprocess.check_output([cli, "metrics", node_artifact]))
    assert metrics["total_tokens"] == 52
    assert metrics["cost_usd"] == 0.002
    [roundtrip] = from_otlp(json.loads(returned.read_text()))
    assert roundtrip["events"][1]["attributes"]["input_tokens"] == 40
    assert roundtrip["events"][1]["attributes"]["api_key"] == "[REDACTED]"
print("Python ↔ Rust ↔ Node artifacts, metrics and OTel contracts passed")
