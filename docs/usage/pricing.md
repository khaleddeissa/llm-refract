# Pricing and invoice reconciliation

Recorded usage is evidence; token prices are configured estimates. Refract never invents a missing
count or assumes an unpriced model is free. Provider invoices remain the billing authority.

## Automatically refresh an approved catalog

Python's `refract.pricing.PriceFeed` refreshes an operator-selected HTTPS JSON feed outside model calls:

```python
from refract.pricing import PriceFeed
import refract

feed = PriceFeed("https://prices.example.com/refract.json", interval=3600, max_age=86400)
feed.start()
handle = refract.instrument_openai(pricing=feed.pricing("openai"))
# Use the application normally. Stop observation and the refresh thread on shutdown.
handle.uninstrument()
feed.close()
```

The feed is a versioned USD catalog with a past, timezone-aware `effective_at`. Rates are dollars per
million tokens, keyed by `provider/model`. For example, these are **synthetic demonstration rates**:

```json
{
  "version": "example-1",
  "effective_at": "2026-01-01T00:00:00Z",
  "currency": "USD",
  "models": {
    "local/demo": { "input_per_million": 1, "output_per_million": 2 }
  }
}
```

Optional rates are `cache_read_per_million` and `cache_write_per_million`. Payloads are capped at 1 MiB;
redirects, embedded URL credentials, negative/non-finite rates and future effective dates are rejected.
A failed refresh retains the last validated catalog until `max_age`, then new estimates become unknown.
Monitor `last_error`. Existing recordings are immutable and retain their original estimates. Archive the
catalog version used by your deployment alongside release/configuration metadata. There is no built-in
scraper or promise that a public price list includes your negotiated rates, regional prices or discounts.
Node accepts explicit `pricing` mappings on instrumentation; refresh those from your application's
approved configuration source.

## Reconcile explicit invoice mappings

```python
from refract.pricing import reconcile

report = reconcile(
    [recording],
    "run_id,event_id,currency,amount\nrun_123,evt_456,USD,0.012\n",
)
```

`amount` is a decimal billed USD amount, including negative credits when appropriate. Import/normalize
your vendor's billing export into this schema and associate actual request IDs with recording event IDs
in your application. The report contains matched estimates and charges, differences, unmatched invoice
rows and unbilled events. Unknown recorded costs remain unknown; reconciliation does not fabricate usage
or infer request mappings. It does not retrieve invoices or change historical snapshots.

Run the [local pricing example](../../examples/python/pricing.py) without any provider account.
