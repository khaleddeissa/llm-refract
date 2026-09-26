"""Run with uv run python examples/python/rag/record.py."""

from pathlib import Path

import refract

Path(".examples").mkdir(exist_ok=True)

with refract.run("policy-rag", path=".examples/rag.rfr", fail_open=False, metadata={"environment": "example"}):
    retrieved = refract.event(
        type="retrieval",
        name="Search policy",
        input={"query": "returns"},
        output={"documents": [{"id": "returns-v1", "text": "Returns within 30 days."}]},
    )
    refract.event(
        type="generation",
        name="Answer with citation",
        parent_id=retrieved,
        output={"answer": "You have 30 days.", "citations": ["returns-v1"]},
        attributes={"provider": "demo", "model": "recorded"},
    )
print("Wrote .examples/rag.rfr")
