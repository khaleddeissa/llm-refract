"""Exchange Python, Node and Rust text artifacts; exercise legacy conversion."""

import json
import subprocess
import tempfile
from pathlib import Path

from refract.artifact import pack, unpack

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
print("Python ↔ Rust ↔ Node artifact contract passed")
