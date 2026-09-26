# Automatic provider instrumentation

Run `npm run build --workspace packages/typescript` then
`node examples/typescript/instrumented/record.mjs` from the repository root.
The example uses local provider-shaped fixtures: no SDK installation, network, API key, or billing.
It records an orchestrator span, an OpenAI Responses call, and an Anthropic streaming call. Each
model event automatically contains provider/model, usage and duration. Streaming captures TTFT and
completion state; the OpenAI fixture has explicit example pricing so estimated cost is reproducible.
The rates are synthetic test values, not current provider prices.

The generated text recording goes to `.examples/instrumented-<timestamp>.rfr`. The graph has one
orchestrator parent with two generation children. To submit the same recording through the batching
exporter, set `REFRACT_ENDPOINT=http://localhost:8000` and optionally `REFRACT_API_KEY`.
Failed batches remain in `.examples/spool` and recover when the next exporter starts.

In a real application replace the fixture clients with your installed OpenAI/Anthropic clients and
leave `instrumentOpenAI(client)` / `instrumentAnthropic(client)` in place. See the
[TypeScript guide](../../../docs/usage/typescript.md) for lifecycle and compatibility limits.
