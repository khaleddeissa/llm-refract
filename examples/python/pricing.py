"""Offline synthetic pricing/reconciliation example; no network or actual billing data."""

import json
from types import SimpleNamespace

import refract
from refract.pricing import PriceCatalog, reconcile

catalog = PriceCatalog(
    {
        "version": "synthetic-1",
        "effective_at": "2026-01-01T00:00:00Z",
        "currency": "USD",
        "models": {"local/demo": {"input_per_million": 1, "output_per_million": 2}},
    }
)
client = SimpleNamespace(
    generate=lambda **kwargs: {"usage": {"input_tokens": 100, "output_tokens": 50}, "text": "demo"}
)
handle = refract.instrument_custom(
    client, "generate", provider="local", pricing=catalog.provider("local")
)
with refract.run("synthetic-pricing") as run:
    client.generate(model="demo")
recording = run.snapshot()
event_id = recording["events"][0]["id"]
# The fabricated bill is 0.00025 USD; the configured estimate is 0.00020 USD.
invoice = f"run_id,event_id,currency,amount\n{recording['id']},{event_id},USD,0.00025\n"
print(json.dumps(reconcile([recording], invoice), indent=2))
handle.uninstrument()
