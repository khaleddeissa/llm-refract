"""Trusted JSON-stdio executor for the checked-in customer-support example."""

import json
import sys

request = json.load(sys.stdin)
event, prefix = request["event"], request["context"]
if event["type"] != "generation":
    raise ValueError("This executor only implements the answer-generation step")
days = prefix["events"][0]["output"]["days"]
print(
    json.dumps(
        {
            "output": {"text": f"You can return your order within {days} days."},
            "attributes": {"provider": "local-example", "model": event["attributes"]["model"]},
        }
    )
)
