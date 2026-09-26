# Optional model measurements (execution v1)

Measurements use existing `event.attributes`; they do not change the canonical run schema. A missing
measurement is unknown. Producers must not use zero to stand in for missing usage or pricing.

| Attribute                                 | Representation                                                                          |
| ----------------------------------------- | --------------------------------------------------------------------------------------- |
| `provider`, `model`                       | Strings describing the actual endpoint family and selected model/deployment             |
| `input_tokens`, `output_tokens`           | Nonnegative integer counts, using the provider's usage definition                       |
| `cache_read_tokens`, `cache_write_tokens` | Nonnegative integer cached-input counts, when reported                                  |
| `total_tokens`                            | Nonnegative integer provider-aware total; caches may already be included in input usage |
| `cost_usd`                                | Finite nonnegative USD estimate under an explicitly configured pricing policy           |
| `ttft_ms`                                 | Finite nonnegative milliseconds until the first observed output token/chunk             |

For OpenAI-compatible usage, cached input is a subset of input tokens. Anthropic usage can report
cache read/write separately from uncached input. Producers normalize `total_tokens` accordingly so
aggregators do not assume all providers share a billing model. Default total fallback is input plus
output; cache fields are never blindly added a second time.

`event.duration_ms` measures event elapsed time. Run wall time is `ended_at - started_at`. The sum of
event durations includes concurrent/nested spans and must not be labelled wall latency. Rerun prefix
measurements describe retained history; rerun suffix measurements must come from the new execution.

Generation-event aggregation exposes measurement/pricing coverage alongside sums. Unknown or partial
coverage fails requested evaluation budgets rather than presenting an incomplete estimate as a full
cost. Numeric token fields are exempt from key-based credential redaction; string secrets under those
keys and unrelated credentials such as `access_token` are still redacted.
