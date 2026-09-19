"""A fresh demo application execution to compare with examples/artifacts/demo.rfr."""

from pathlib import Path

import refract

Path(".examples").mkdir(exist_ok=True)
with refract.run("customer-support", path=".examples/actual.rfr"):
    days = 30
    retrieval = refract.event(
        type="retrieval",
        name="Find return policy",
        input={"query": "return window"},
        output={"days": days},
    )
    answer = f"You can return your order within {days} days."
    refract.event(
        type="generation",
        name="Draft answer",
        parent_id=retrieval,
        input={"prompt": "Summarize the return policy."},
        output={"text": answer},
        attributes={"provider": "demo", "model": "recorded-example"},
    )
