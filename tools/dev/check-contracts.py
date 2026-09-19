"""Validate the checked-in canonical fixture against the published JSON Schemas."""

import runpy
from pathlib import Path

suite = runpy.run_path(
    str(Path(__file__).resolve().parents[2] / "tests/contract/test_contracts.py")
)
suite["test_shared_fixture"]()
suite["test_python_sdk_matches_wire_contract"]()
print("Execution contracts are consistent")
