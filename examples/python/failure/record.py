"""Capture a failure; exception messages are not automatically recorded."""

import refract

try:
    with refract.run("failed-lookup", path="failure.rfr"):
        refract.event(
            type="tool.call",
            name="Lookup customer",
            input={"api_key": "demo-secret"},
            output={"found": False},
        )
        raise LookupError("No matching customer")
except LookupError:
    print("Wrote failure.rfr with failed status and a redacted input")
