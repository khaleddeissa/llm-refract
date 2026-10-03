# OpenTelemetry and Langfuse

Refract bridges portable recordings to and from OpenTelemetry traces. Python and Node support
OTLP/HTTP JSON conversion and explicit export. Python also imports completed OpenTelemetry SDK spans.
The service also receives native OTLP JSON/protobuf over HTTP and OTLP gRPC on its API port.

## Convert recordings offline

```python
import json
from pathlib import Path
from refract.artifact import pack, unpack
from refract.otel import to_otlp, from_otlp

recording = unpack(Path("examples/artifacts/demo.rfr").read_bytes())
document = to_otlp(recording, service_name="my-assistant")
Path(".examples").mkdir(exist_ok=True)
Path(".examples/trace.json").write_text(json.dumps(document, indent=2))
for index, run in enumerate(from_otlp(document)):
    Path(f".examples/imported-{index}.rfr").write_bytes(pack(run))
```

```typescript
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { unpack, pack, toOtlp, fromOtlp } from "@llm-refract/sdk";

const recording = unpack(await readFile("examples/artifacts/demo.rfr"));
const document = toOtlp(recording);
await mkdir(".examples", { recursive: true });
for (const [index, run] of fromOtlp(document).entries()) {
  await writeFile(`.examples/imported-${index}.rfr`, pack(run));
}
```

A document can contain multiple traces, producing multiple recordings. Refract-generated attributes
preserve recording/event identifiers, payloads and event types; generic traces map into the canonical
schema and retain OTel resource/span metadata. Token, model and provider attributes are normalized
where supported. Arbitrary framework-specific attributes are not automatically understood as usage.
Redaction still applies to imported and exported recordings.

## Send to an OTel collector

```python
from refract.otel import export_otlp

# Use a trusted collector endpoint; supply deployment-specific headers if required.
export_otlp(recording, "http://127.0.0.1:4318/v1/traces", timeout=10)
```

```typescript
import { exportOtlp } from "@llm-refract/sdk";
await exportOtlp(recording, "http://127.0.0.1:4318/v1/traces");
```

These calls send JSON synchronously/when awaited and report transport errors or partial rejection.
They are separate from the background exporters that deliver to Refract's `/v1/runs/batch` API.
Use the appropriate OTel exporter/collector in your application when you need batching, retries,
protobuf transport or routing to multiple observability backends. Use HTTPS beyond trusted local
networks. Configure destination credentials outside recording payloads.

## Import completed Python SDK spans

Install `llm-refract[otel]` to use OpenTelemetry SDK objects. Pass finished spans from an exporter
or an in-memory test exporter:

```python
from refract.otel import from_spans

# finished_spans = your_in_memory_exporter.get_finished_spans()
# for recording in from_spans(finished_spans):
#     send_to_refract_or_save_as_artifact(recording)
```

`from_spans` converts completed span objects through the same OTLP mapping. It does not install a
global tracer provider or intercept all frameworks automatically. Collect a complete trace before
conversion when parent/child structure matters; this bridge is not a distributed trace assembler.

## Langfuse

Langfuse accepts OTLP/HTTP JSON at `/api/public/otel/v1/traces`. The Refract adapters add Langfuse
observation attributes and authenticate with your project's public/secret key pair. They include
`x-langfuse-ingestion-version: 4`. See the upstream
[Langfuse OpenTelemetry contract](https://langfuse.com/integrations/native/opentelemetry).

```python
import os
from refract.integrations.langfuse import to_langfuse, export_langfuse

# Conversion is offline; only export_langfuse makes a request.
langfuse_document = to_langfuse(recording)
export_langfuse(
    recording,
    os.environ["LANGFUSE_BASE_URL"],
    public_key=os.environ["LANGFUSE_PUBLIC_KEY"],
    secret_key=os.environ["LANGFUSE_SECRET_KEY"],
)
```

```typescript
import { exportLangfuse } from "@llm-refract/sdk";
await exportLangfuse(recording, process.env.LANGFUSE_BASE_URL!, {
  publicKey: process.env.LANGFUSE_PUBLIC_KEY!,
  secretKey: process.env.LANGFUSE_SECRET_KEY!,
});
```

Use a completed, nonempty recording. Generation, tool and retrieval events become corresponding
observations; model, usage and explicitly measured cost are included when present. Recording metadata
can include `session_id`, `user_id`, `environment`, `release`, `version` and `tags`. Export each finished
recording once; retries/repeated imports are not guaranteed to deduplicate in Langfuse. The adapter
exports recordings, not Langfuse project configuration, prompts, datasets or existing trace history.

From a checkout, preview a converted recording without network access:

```bash
uv run python examples/python/providers/langfuse_export.py examples/artifacts/demo.rfr
# With credentials configured, add --send to explicitly export.
node examples/typescript/providers/langfuse.mjs --help
```

## Contracts and boundaries

`make test-contract` verifies Python → Node → Rust artifact/measurement interoperability and OTLP JSON
round trips. SDK suites test generic spans, redaction, Langfuse mappings and HTTP failures/partial
rejection. A live collector or Langfuse deployment is a separate integration check; tests do not
silently send your recordings to a cloud service.

See [provider integrations](providers.md) for LangChain and local models, and
[production operation](../production.md) for Refract service deployment and credential separation.

## Native ingestion and distributed assembly

Point an OTel exporter at `http://127.0.0.1:8000/v1/traces` for HTTP JSON (`application/json`) or
protobuf (`application/x-protobuf`). OTLP gRPC uses the same port and the standard
`opentelemetry.proto.collector.trace.v1.TraceService/Export` method. Supply `Authorization: Bearer KEY`
as HTTP headers or gRPC metadata; a writer role is required. Use HTTPS ingress with HTTP/2 forwarding
for production gRPC. Health and trace traffic use the same service deployment.

```yaml
# OpenTelemetry Collector exporter fragment; localhost is the collector's network namespace.
exporters:
  otlphttp/refract:
    endpoint: http://127.0.0.1:8000
    headers:
      Authorization: "Bearer ${env:REFRACT_API_KEY}"
```

The server redacts before persistence and assembles spans by trace ID **within the authenticated scope**.
Late parents produce a new immutable content-addressed snapshot containing the updated graph; they do
not mutate earlier recordings. Identical retries reuse evidence; conflicting content for the same span
ID and cyclic parentage are rejected. Missing parents remain explicit external references until seen.
A request with multiple traces may persist earlier traces before a later invalid trace fails; retries
are content-checked. Exporters should inspect failures and retry valid data.

Limits: 16 MiB per request, 1,000 spans per trace batch, 10,000 spans/32 MiB per assembled trace and
10,000 pending trace assemblies per scope. Assembly state expires after 24 hours during maintenance;
recording snapshots follow normal run retention. Preserve upstream trace/span IDs for cross-process causality; a collector cannot recover spans
that were never emitted. Local tests exercise actual protobuf/gRPC transport, auth, out-of-order spans,
retry deduplication and cycle rollback. The offline SDK bridges remain stateless converters.

## Logs and metrics

The same authenticated port accepts all three OTLP signals. Send logs to `/v1/logs` and metrics to
`/v1/metrics` with `application/json` or `application/x-protobuf`. Standard gRPC
`LogsService/Export` and `MetricsService/Export` are available alongside `TraceService/Export`.
Use a writer key for ingestion and a reader key for queries. Collector pipelines for traces, logs
and metrics can share the `otlphttp/refract` exporter above.

Resource, instrumentation-scope and data-point attributes are normalized before redaction. Logs retain
severity, timestamps and trace/span IDs; metrics retain gauge, sum, histogram, exponential histogram
or summary data, including temporality and exemplars. They are stored as telemetry records, separately
from replayable `.rfr` executions. Refract preserves measurements; it does not calculate rates or
aggregate metric windows automatically. Exact retries deduplicate by normalized content; identical
records within one export retain their multiplicity. Repeated identical records across separate
exports collapse, so supply real timestamps for distinct observations.

```python
from refract import RefractClient

client = RefractClient("http://127.0.0.1:8000")
page = client.telemetry(kind="logs", trace_id="0123456789abcdef0123456789abcdef")
metrics = client.telemetry(kind="metrics", limit=50)
```

```typescript
const page = await client.telemetry("logs", {
  trace_id: "0123456789abcdef0123456789abcdef",
});
const metrics = await client.telemetry("metrics", { limit: 50 });
```

`GET /v1/telemetry?kind=logs&limit=50&offset=0` returns `records` and `next_offset`. Log queries can
filter by trace ID. The Inspector's **OpenTelemetry logs and metrics** panel shows records and their
attributes; MCP exposes `telemetry_records`. Telemetry uses tenant isolation, encryption/key rotation
and the configured receipt-time retention period. Manual `/v1/admin/retention` also expires telemetry.
Pages have at most 100 records/12 MiB; ingestion allows 10,000 records/16 MiB per export and 1 MiB per
normalized record. Invalid batches roll back. See [runnable local examples](../../examples/otel/README.md).
