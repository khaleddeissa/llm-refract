# Local OTLP logs and metrics

Run a local Refract server, then send one synthetic completion log and one cumulative request counter:

```bash
uv run python examples/otel/ingest.py
```

Set `REFRACT_SERVER_URL` and `REFRACT_API_KEY` for an authenticated deployment. The key needs writer
access. The counter value is deliberately `1`; timestamps come from the local clock. No model or
cloud account is contacted. The fixture `api_key` attribute demonstrates redaction.

Open the Inspector's **OpenTelemetry logs and metrics** panel to browse the records.
For real workloads, use a standard OTel exporter or Collector with batching and retries; see
[OTLP transports and configuration](../../docs/usage/otel.md#logs-and-metrics).
