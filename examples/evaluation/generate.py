"""Build a small evaluation dataset from fresh local application runs; no provider calls."""

import json
from pathlib import Path

import refract

root = Path(".examples/evaluation")
root.mkdir(parents=True, exist_ok=True)


def record(path: Path, days: int) -> None:
    with refract.run("refund-policy", path=path, fail_open=False):
        refract.event(
            type="generation",
            name="answer",
            output={"text": f"Returns within {days} days."},
            attributes={"provider": "local-demo", "model": "policy-function"},
        )


# Reference behavior and the application candidate are run separately.
record(root / "baseline.rfr", 30)
record(root / "candidate.rfr", 30)
record(root / "regression.rfr", 14)
(root / "dataset.json").write_text(
    json.dumps(
        {
            "version": 1,
            "cases": [
                {"name": "equivalent", "baseline": "baseline.rfr", "candidate": "candidate.rfr"},
                {
                    "name": "policy-regression",
                    "baseline": "baseline.rfr",
                    "candidate": "regression.rfr",
                },
            ],
        },
        indent=2,
    )
    + "\n"
)
print("Wrote .examples/evaluation; evaluation intentionally reports one factual regression.")
