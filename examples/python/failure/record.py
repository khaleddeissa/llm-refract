"""Capture a failure; exception messages are not automatically recorded."""

from pathlib import Path

import refract

Path(".examples").mkdir(exist_ok=True)

try:
    with refract.run("failed-lookup", path=".examples/failure.rfr", fail_open=False):
        refract.event(
            type="tool.call",
            name="Lookup customer",
            input={"api_key": "demo-secret"},
            output={"found": False},
        )
        raise LookupError("No matching customer")
except LookupError:
    print("Wrote .examples/failure.rfr with failed status and a redacted input")
