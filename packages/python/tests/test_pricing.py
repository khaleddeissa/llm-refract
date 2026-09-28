import json
import time
from types import SimpleNamespace

import pytest

import refract
from refract.pricing import PriceCatalog, PriceFeed, reconcile


def catalog(rate=1):
    return {
        "version": "fixture-v1",
        "currency": "USD",
        "effective_at": "2026-01-01T00:00:00Z",
        "models": {"local/model": {"input_per_million": rate, "output_per_million": rate}},
    }


def test_feed_updates_live_pricing_view_and_stale_prices_become_unknown():
    documents = iter([json.dumps(catalog()), json.dumps(catalog(2)), "invalid"])
    feed = PriceFeed("https://prices.invalid/feed", fetch=lambda: next(documents))
    client = SimpleNamespace(
        generate=lambda **kwargs: {"usage": {"input_tokens": 100, "output_tokens": 100}}
    )
    handle = refract.instrument_custom(
        client, "generate", provider="local", pricing=feed.pricing("local")
    )
    assert feed.refresh()
    client.generate(model="model")
    assert handle.completed_runs[-1]["events"][0]["attributes"]["cost_usd"] == 0.0002
    assert feed.refresh()
    client.generate(model="model")
    assert handle.completed_runs[-1]["events"][0]["attributes"]["cost_usd"] == 0.0004
    assert not feed.refresh()
    feed._updated = time.monotonic() - feed.max_age - 1
    client.generate(model="model")
    assert "cost_usd" not in handle.completed_runs[-1]["events"][0]["attributes"]
    handle.uninstrument()


def test_reconciliation_keeps_unknown_costs_and_unmatched_invoice_rows():
    runs = [
        {
            "id": "r",
            "events": [{"id": "a", "attributes": {"cost_usd": 0.1}}, {"id": "b", "attributes": {}}],
        }
    ]
    report = reconcile(
        runs, "run_id,event_id,currency,amount\nr,a,USD,0.12\nr,b,USD,0.20\nother,c,USD,1\n"
    )
    assert report["matched"][0]["difference_usd"] == "0.02"
    assert report["matched"][1]["difference_usd"] is None
    assert len(report["unmatched_invoice"]) == 1
    with pytest.raises(ValueError):
        reconcile(runs, "run_id,event_id,currency,amount\nr,a,EUR,1\n")
    with pytest.raises(ValueError):
        PriceCatalog(catalog(-1))
