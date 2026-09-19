import refract

with refract.run("customer-support", path="example.rfr"):
    lookup = refract.event(type="retrieval", name="Find policy", output={"return_days": 30})
    refract.event(
        type="generation",
        name="Draft answer",
        parent_id=lookup,
        input={"prompt": "What is the return policy?"},
        output={"text": "Returns are accepted within 30 days."},
        attributes={"provider": "demo", "model": "recorded-example"},
    )
print("Wrote example.rfr")
