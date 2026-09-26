# TypeScript / npm application mode

`@llm-refract/sdk` targets Node.js applications, services, and agents. It uses `AsyncLocalStorage`,
filesystem and crypto APIs; it is not a browser SDK. Install with `npm install @llm-refract/sdk`;
see [development](../development.md) to build from source.

## Automatic provider capture

Pass an existing client into the opt-in wrapper. No provider packages are runtime dependencies of
Refract. Wrappers use the client's configured credentials, endpoint, region and model. They capture
calls without routing inference through the Refract server. Each returns a function restoring the
original methods.

| Provider                           | Wrapper                             | Captured SDK methods                                                     |
| ---------------------------------- | ----------------------------------- | ------------------------------------------------------------------------ |
| OpenAI                             | `instrumentOpenAI(client)`          | `responses.create`, `chat.completions.create`                            |
| Azure OpenAI                       | `instrumentAzureOpenAI(client)`     | Same methods on an application-configured `AzureOpenAI` client           |
| Anthropic                          | `instrumentAnthropic(client)`       | `messages.create`                                                        |
| Gemini                             | `instrumentGemini(client)`          | `@google/genai` `models.generateContent`, `models.generateContentStream` |
| Vertex AI                          | `instrumentVertex(client)`          | Same Google Gen AI methods with the client configured for Vertex         |
| Amazon Bedrock                     | `instrumentBedrock(client)`         | AWS SDK v3 `send(new ConverseCommand(...))` / `ConverseStreamCommand`    |
| Custom / local / OpenAI-compatible | `instrumentCustom(client, options)` | Application-selected methods and response normalization                  |

The wrapper names describe supported SDK contracts, not a list of approved models. Choose any model
supported by that endpoint and method. Provider authentication stays with your application; Refract
has no separate provider API-key registry. The same instrumentation works with offline `.rfr` output
or authenticated remote ingestion.

```typescript
import OpenAI from "openai"; // installed by your application
import { instrumentOpenAI, refract } from "@llm-refract/sdk";

const client = new OpenAI();
const restore = instrumentOpenAI(client);
try {
  await refract.run(
    "answer",
    async () => {
      return await client.responses.create({
        model: process.env.OPENAI_MODEL!,
        input: "Explain execution replay",
      });
    },
    { path: "answer.rfr", onError: console.error },
  );
} finally {
  restore();
}
```

Use `instrumentAnthropic(client)` for Anthropic. Capture includes request input, response (including
provider-requested tool calls), model/provider, duration, tokens, cache usage, and failures. Refract
records these requests; it does not execute provider-requested tools. Consume async-iterable streams
inside `refract.run`: output text, tool-call fragments, TTFT, final usage, partial consumption, and
stream errors are captured while chunks are returned unchanged. Chat Completions usage is available
only when the provider returns it; request `stream_options: { include_usage: true }` when supported.
OpenAI Responses and Anthropic streaming usage follow the
[Responses event schema](https://platform.openai.com/docs/api-reference/responses-streaming) and
[Anthropic streaming schema](https://platform.claude.com/docs/en/build-with-claude/streaming).

Wrappers support normal `await client.*.create(...)`, `for await` streams, and `.withResponse()`
with response metadata preserved. `.asResponse()` leaves the raw body unread and records latency
without parsing response content (`response_body_captured: false`). Convenience `.stream()` methods
are not wrapped; use `.create()` for capture. Instrumentation requires an active run; calls outside
a run or in a sampled-out run return the original provider result untouched. Patching the same
client repeatedly is unnecessary.

Metrics live in event attributes (`input_tokens`, `output_tokens`, `cache_read_tokens`,
`cache_write_tokens`, `total_tokens`, `ttft_ms`). Exact numeric usage keys survive secret-key
redaction; arbitrary credential keys containing `token` remain redacted. Cost is unknown unless
application-maintained rates are supplied, expressed in USD per million tokens:

```typescript
const restore = instrumentOpenAI(client, {
  pricing: { "your-model": { input: 1, output: 3, cachedInput: 0.1 } },
  maxOutputChars: 1_000_000,
});
```

Those rates are illustrative. `cost_usd` is marked `cost_estimated`; cache creation uses the supplied
input rate and does not model every provider's pricing tier. Streaming text is bounded by
`maxOutputChars`, with `output_truncated` recorded. TTFT measures first text, not time to first
metadata chunk. Consume or cancel streams before the run finishes.

## Gemini, Vertex, Azure and Bedrock

Install the provider SDKs used by your application; none is installed transitively by Refract.
These examples perform real inference when called, so select your own model/deployment and supply
credentials through the provider's normal configuration.

```typescript
import { GoogleGenAI } from "@google/genai";
import { instrumentGemini, instrumentVertex, refract } from "@llm-refract/sdk";

const gemini = new GoogleGenAI({ apiKey: process.env.GEMINI_API_KEY });
const restoreGemini = instrumentGemini(gemini);
const vertex = new GoogleGenAI({
  vertexai: true,
  project: process.env.GOOGLE_CLOUD_PROJECT,
  location: process.env.GOOGLE_CLOUD_LOCATION,
});
const restoreVertex = instrumentVertex(vertex);
try {
  await refract.run(
    "google-answer",
    async () => {
      for await (const chunk of await gemini.models.generateContentStream({
        model: process.env.GEMINI_MODEL!,
        contents: "Explain portable execution traces",
      }))
        console.log(chunk.text);
      await vertex.models.generateContent({
        model: process.env.VERTEX_MODEL!,
        contents: "Summarize execution replay",
      });
    },
    { path: "google-answer.rfr" },
  );
} finally {
  restoreGemini();
  restoreVertex();
}
```

Google capture uses [Gen AI generation methods](https://googleapis.github.io/js-genai/release_docs/classes/models.Models.html).
Streaming captures visible text and function calls. Usage includes cached input, generated candidate
and thought token counts, and the provider's total when supplied. Thought text is excluded from the
stream's visible-text summary. Legacy `@google/generative-ai` and `@google-cloud/vertexai` clients,
chat helpers, embeddings and image/video methods are outside these automatic wrappers; use the
custom adapter or explicit spans for those APIs.

```typescript
import { AzureOpenAI } from "openai";
import {
  BedrockRuntimeClient,
  ConverseStreamCommand,
} from "@aws-sdk/client-bedrock-runtime";
import {
  instrumentAzureOpenAI,
  instrumentBedrock,
  refract,
} from "@llm-refract/sdk";

const azure = new AzureOpenAI({
  endpoint: process.env.AZURE_OPENAI_ENDPOINT,
  apiKey: process.env.AZURE_OPENAI_API_KEY,
  apiVersion: process.env.AZURE_OPENAI_API_VERSION,
});
const bedrock = new BedrockRuntimeClient({ region: process.env.AWS_REGION });
const restoreAzure = instrumentAzureOpenAI(azure);
const restoreBedrock = instrumentBedrock(bedrock);
try {
  await refract.run(
    "cloud-answer",
    async () => {
      await azure.chat.completions.create({
        model: process.env.AZURE_OPENAI_DEPLOYMENT!,
        messages: [{ role: "user", content: "Explain recording and replay" }],
      });
      const response = await bedrock.send(
        new ConverseStreamCommand({
          modelId: process.env.BEDROCK_MODEL_ID!,
          messages: [
            { role: "user", content: [{ text: "Explain model comparison" }] },
          ],
        }),
      );
      for await (const chunk of response.stream!)
        console.log(chunk.contentBlockDelta?.delta?.text ?? "");
    },
    { path: "cloud-answer.rfr" },
  );
} finally {
  restoreAzure();
  restoreBedrock();
}
```

Bedrock captures [Converse stream events](https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_ConverseStream.html),
including nested text/tool deltas, usage metadata and exception events. Request options, abort signals,
AWS response metadata and yielded chunks are preserved. Callback-style `send` and commands other than
`ConverseCommand` / `ConverseStreamCommand` pass through without automatic capture. In particular,
model-specific `InvokeModel` bodies need a custom adapter. Configuring IAM, Azure identity or Google
application-default credentials remains the application's responsibility.

## Custom and local model backends

For an OpenAI-compatible server, use `instrumentOpenAI` on the already-configured SDK client or
`instrumentCustom` to record a specific provider label. Custom adapters also work with an in-process
model, HTTP wrapper, Ollama client, or your own API contract:

For a local OpenAI-compatible service, configure the provider client with its inference URL:

```typescript
const client = new OpenAI({
  baseURL: process.env.LLM_BASE_URL!, // e.g. http://localhost:11434/v1 for Ollama
  apiKey: process.env.LLM_API_KEY ?? "local",
});
const restore = instrumentOpenAI(client);
```

Use Chat Completions and a model already installed/served by that endpoint. The
[local endpoint example](../../examples/typescript/providers/README.md) covers Ollama and vLLM,
stream consumption and authenticated recording. Endpoint compatibility determines available methods
and options; Refract does not download weights or convert between provider inference protocols.
`LLM_BASE_URL` is the inference service; `REFRACT_ENDPOINT` is the independent recording service.

For a non-OpenAI contract, choose the request and response mapping explicitly:

```typescript
import { instrumentCustom, refract } from "@llm-refract/sdk";

const local = {
  async generate(prompt: string, model: string) {
    // Replace this fixture with your local model call.
    return { answer: "Portable traces", generated: 2 };
  },
};
const restore = instrumentCustom(local, {
  provider: "my-local-model",
  methods: [["generate"]],
  request: (args) => ({
    model: String(args[1]),
    input: { prompt: String(args[0]) },
  }),
  normalize: (value) => {
    const response = value as { answer: string; generated: number };
    return {
      output: response.answer,
      usage: { output_tokens: response.generated },
    };
  },
});
try {
  await refract.run(
    "local-answer",
    () => local.generate("What is Refract?", "my-model"),
    { path: "local-answer.rfr" },
  );
} finally {
  restore();
}
```

The normalizer receives `{ streaming: boolean }` as its second argument. For async-iterable responses,
return `text` deltas, `toolCalls`, `usage` and optionally `failed`; final usage snapshots overwrite
earlier fields rather than summing cumulative provider counters. Set `streamKey` when the iterable
is nested inside a response envelope. Custom token counts use `input_tokens`, `output_tokens`,
`total_tokens`, `cache_read_input_tokens` and `cache_creation_input_tokens`; include cache usage in
input totals. Normalization failures fail open and mark `capture_incomplete`; they never replace
provider results or errors. Callback/event-emitter APIs require an application-owned async adapter.
This extensibility supports other providers without claiming automatic coverage of every SDK method.

## LangChain and other frameworks

Wrap an application-owned runnable or chain in a span to capture its input, result and errors:

```typescript
await refract.run(
  "langchain-request",
  () =>
    refract.span(
      { type: "decision", name: "question-chain", input: { question } },
      () => chain.invoke({ question }),
    ),
  {
    endpoint: process.env.REFRACT_ENDPOINT,
    apiKey: process.env.REFRACT_API_KEY,
  },
);
```

Here `chain` is your already-configured LangChain runnable and `question` is application input.
This records the outer call; it does not discover LangChain's internal nodes. Instrument accessible
provider clients for nested model spans, add explicit spans around tools, or import existing OTLP
traces using `fromOtlp`. The native callback handler is currently available in the Python SDK;
TypeScript has no automatic LangChain callback adapter. Existing framework credentials, streaming,
and orchestration remain under application control. Do not instrument the same model call through
multiple layers if you want one generation event per inference.

## Nested spans and explicit events

```typescript
import { refract } from "@llm-refract/sdk";

await refract.run(
  "agent",
  () =>
    refract.span(
      { type: "tool.call", name: "planner", replay_policy: "MOCK" },
      async () => {
        refract.event({
          type: "retrieval",
          name: "documents",
          output: ["guide"],
        });
        // Automatically instrumented provider calls also inherit the planner parent.
        return "done";
      },
    ),
  { path: "agent.rfr" },
);
```

Concurrent runs and spans retain separate async contexts. Background work must finish inside the
run callback; late explicit events are rejected. Application exceptions always propagate unchanged.
`onComplete(execution)` receives a cloned snapshot after export. `pack` / `unpack` write/read the text
artifact profile and verify checksums; Rust additionally reads legacy ZIP artifacts.

## Background ingestion, durability and sampling

```typescript
import { BatchExporter, refract } from "@llm-refract/sdk";

const exporter = new BatchExporter({
  endpoint: "http://localhost:8000",
  apiKey: process.env.REFRACT_API_KEY,
  batchSize: 32,
  maxQueueSize: 1024,
  flushIntervalMs: 1000,
  maxAttempts: 3,
  spoolDirectory: ".refract-spool",
  onError: console.error,
});
try {
  await refract.run("request", async () => "answer", {
    exporter,
    sampleRate: 0.25,
    onError: console.error,
  });
} finally {
  await exporter.shutdown();
}
```

The exporter sends `{ runs: [...] }` to `POST /v1/runs/batch` with optional bearer authentication.
Completed snapshots enter a bounded queue; overflow is counted in `exporter.stats.dropped`.
Sampling decides once per run and never skips application logic. With a spool directory, redacted
snapshots are written atomically before `run` returns; network submission occurs in the background.
Use one exporter per spool directory, retain its filesystem across restarts, and restrict access.
This is local retry durability, not a replicated queue or filesystem encryption.

Transient failures and HTTP 429 retry with exponential delay. Failed batches remain queued for the
next flush. The server treats identical repeated snapshots as idempotent, so a lost response does
not duplicate stored runs. Permanent invalid batches stay queued/spooled for diagnosis and can
block later export; inspect `onError` and use validated captures. `shutdown()` stops the timer and
waits for the final bounded retry cycle. Failed snapshots remain on disk; without a spool they are
lost when the process exits. Startup loads at most `maxQueueSize` existing snapshots.

Recording and export fail open by default: an unavailable Refract server does not fail a successful
application request. `onError` reports synchronous recording/export failures; the exporter's
`onError` reports background failures. Set `failOpen: false` for strict local recording or tests.
Direct `{ endpoint, apiKey }` run options still submit synchronously, with a 10-second timeout;
omit `endpoint` when using a batch exporter to avoid duplicate submission. `path` writes with
no overwrite, and can accompany either export mode.

## OpenTelemetry bridge

`toOtlp`, `fromOtlp`, and `exportOtlp` translate between canonical executions and
[OTLP/HTTP JSON traces](https://opentelemetry.io/docs/specs/otlp/#json-protobuf-encoding).
There is no required OpenTelemetry dependency:

```typescript
import { readFile, writeFile } from "node:fs/promises";
import { unpack, pack, toOtlp, fromOtlp, exportOtlp } from "@llm-refract/sdk";

const execution = unpack(await readFile("answer.rfr"));
await writeFile("trace.json", JSON.stringify(toOtlp(execution), null, 2));
await exportOtlp(execution, "http://localhost:4318");
const [imported] = fromOtlp(JSON.parse(await readFile("trace.json", "utf8")));
await writeFile("imported.rfr", pack(imported));
```

Each nonempty trace becomes a run. The bridge preserves causal parents, payloads, metrics, replay
policy, external parent IDs, span links, and span events. Imported parents are ordered before children;
duplicate IDs and parent cycles are rejected. Unknown native OTel spans become `decision` events,
or `generation` when model attributes are present. All imported/exported data passes redaction.
OTLP integer timestamps use decimal strings; JavaScript event timestamps have millisecond precision.
The bridge does not install global OTel instrumentation or export logs/metrics. `exportOtlp` awaits
the HTTP request and reports collector failures or partial rejection; handle errors explicitly if
calling it in an application request. Use an external OTel Collector for its own retry/backends.

## Langfuse interoperability

`toLangfuse` maps a completed, nonempty recording to Langfuse observations, including parent IDs,
input/output, model, usage and recorded cost. `exportLangfuse` sends it to a cloud or self-hosted
instance using [Langfuse's OTLP endpoint](https://langfuse.com/integrations/native/opentelemetry):

```typescript
import { exportLangfuse, toLangfuse } from "@llm-refract/sdk";

await exportLangfuse(execution, process.env.LANGFUSE_BASE_URL!, {
  publicKey: process.env.LANGFUSE_PUBLIC_KEY!,
  secretKey: process.env.LANGFUSE_SECRET_KEY!,
});
const document = toLangfuse(execution); // inspect or send through your own collector
```

Credentials are request headers, excluded from recording data. Metadata keys `user_id`, `session_id`,
`environment`, `tags`, `version` and `release` are copied to each observation. Missing usage/cost stays
missing. These exports are explicit and awaited; handle failures in your background delivery path.
Avoid sending the same execution through both this exporter and an independent Langfuse instrumentor.
This integration supplies trace data, not Langfuse prompt management, datasets or evaluation APIs.
The [export example](../../examples/typescript/providers/langfuse.mjs) accepts an existing `.rfr` file.
Tests validate request headers, mappings, redaction and rejection handling offline; they do not
assert live cloud acceptance or provisioning.

## Runnable examples and tests

- [Provider fixture + nested graph + streaming](../../examples/typescript/instrumented/README.md):
  `node examples/typescript/instrumented/record.mjs`, no paid requests.
- [All provider contracts](../../examples/typescript/providers/README.md):
  `node examples/typescript/providers/record.mjs`, exercises seven provider labels offline.
- `node examples/typescript/basic/record.mjs`: manual recording.
- `node examples/typescript/concurrent/record.mjs`: concurrent async isolation.
- `npm test --workspace packages/typescript`: artifacts, isolation, provider responses/streams,
  sampling, token preservation, strict/fail-open behavior, retries, queue overflow and spool recovery.

Build the SDK before examples. Generated files live under `.examples/`.
