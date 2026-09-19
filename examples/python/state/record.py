"""Capture a state transition and explicit checkpoint."""

import refract

with refract.run("checkout", path="state.rfr"):
    changed = refract.event(
        type="state.change",
        name="Apply discount",
        input={"cart_total": 120},
        output={"cart_total": 90},
    )
    refract.event(
        type="checkpoint",
        name="Before payment",
        parent_id=changed,
        output={"cart_total": 90, "payment": "not_started"},
        replay_policy="BLOCKED",
    )
print("Wrote state.rfr; BLOCKED checkpoint deliberately prevents recorded playback")
