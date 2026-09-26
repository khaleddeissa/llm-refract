# Model providers and framework integrations

Refract records your application's model calls as portable execution events. Your application owns
model selection, credentials, retries and network configuration. The same recording can be saved as
an offline `.rfr`, inspected locally, or exported to an authenticated production service.

## Supported instrumentation

| Provider or framework                   | Python                                  | Node / TypeScript                      | Covered surface                                                        |
| --------------------------------------- | --------------------------------------- | -------------------------------------- | ---------------------------------------------------------------------- |
| OpenAI                                  | `instrument_openai()`                   | `instrumentOpenAI(client)`             | Responses and Chat Completions, including streaming                    |
| Anthropic                               | `instrument_anthropic()`                | `instrumentAnthropic(client)`          | Messages API and streaming                                             |
| Azure OpenAI                            | `instrument_azure(client)`              | `instrumentAzureOpenAI(client)`        | Configured OpenAI-compatible client                                    |
| Gemini                                  | `instrument_google(client)`             | `instrumentGemini(client)`             | Google Gen AI content generation and streaming                         |
| Vertex AI                               | `instrument_google(client)`             | `instrumentVertex(client)`             | Google Gen AI client configured for Vertex                             |
| Amazon Bedrock                          | `instrument_bedrock(client)`            | `instrumentBedrock(client)`            | Boto3 Converse/ConverseStream; AWS SDK v3 Converse commands            |
| Ollama, vLLM, compatible gateways       | OpenAI or custom adapter                | OpenAI or custom adapter               | Compatible Chat Completions/Responses methods provided by the endpoint |
| Local weights, custom or private models | `instrument_custom(owner, method, ...)` | `instrumentCustom(client, options)`    | Explicit methods and response normalization                            |
| LangChain                               | `langchain_handler()`                   | Explicit SDK spans or OTLP JSON bridge | Python chain, model, retrieval and tool callbacks                      |
| Langfuse                                | `to_langfuse` / `export_langfuse`       | `toLangfuse` / `exportLangfuse`        | Completed recordings exported through OTLP/HTTP JSON                   |

“Any provider” means the canonical event format and custom adapters can represent its calls. It does
not mean every SDK method is automatically instrumented. Bedrock InvokeModel, non-Google Vertex SDKs,
Realtime APIs and framework-specific orchestration need explicit adapters or manual spans. Node
Bedrock instrumentation covers promise/stream commands, not callback-style `send` calls.

Python optional extras install the corresponding dependencies:

```bash
pip install 'llm-refract[openai,anthropic,google,bedrock,langchain,otel]'
```

Install only the extras you need. Node users install `@llm-refract/sdk` plus the provider's own SDK.
These examples describe this checkout; build it locally for features not yet in a published release.

## Configure the client, then record

A custom adapter can record any existing client without changing its endpoint or credentials:

```python
from pathlib import Path
import refract


class LocalModel:
    def generate(self, prompt):
        return {"text": prompt.upper()}  # Offline demonstration; replace with inference.


model = LocalModel()
handle = refract.instrument_custom(
    model,
    "generate",
    provider="custom",
    model="local-demo",
    request=lambda args, kwargs: {"input": args[0]},
)
Path(".examples").mkdir(exist_ok=True)
try:
    with refract.run("local-answer", path=".examples/custom.rfr", fail_open=False):
        model.generate("hello")
finally:
    handle.uninstrument()
```

`request(args, kwargs)` selects recordable prompt/model fields; optional `response(value)` returns a
JSON-friendly copy for recording. For stream methods, set `streaming=True` and normalize each chunk.
Sync methods, awaitables, iterators and async iterators preserve application-owned consumption.
The original provider result is returned. Custom inference usage remains unknown unless you supply it.

With an existing cloud SDK client, use its matching wrapper before the call:

```python
# Each line is an alternative for the corresponding configured client.
openai_handle = refract.instrument_openai()
anthropic_handle = refract.instrument_anthropic()
# azure_handle = refract.instrument_azure(azure_client)
# google_handle = refract.instrument_google(google_or_vertex_client)
# bedrock_handle = refract.instrument_bedrock(boto3_bedrock_runtime_client)
```

OpenAI and Anthropic Python wrappers patch supported SDK resource classes process-wide until
`handle.uninstrument()`. Google supports a supplied client or class-wide installation. Azure and
Bedrock can patch supplied instances. Install once at application startup and restore at shutdown;
do not repeatedly stack wrappers. Consume streams within the recording context to capture final usage.

For Node, wrappers patch the supplied client and return a restore function:

```typescript
import { instrumentCustom, refract } from "@llm-refract/sdk";

const model = {
  generate: async (prompt: string) => ({ text: prompt.toUpperCase() }),
};
const restore = instrumentCustom(model, {
  provider: "custom",
  methods: [["generate"]],
  request: (args) => ({ model: "local-demo", input: String(args[0]) }),
});
try {
  await refract.run("local-answer", () => model.generate("hello"), {
    path: ".examples/custom-node.rfr",
    failOpen: false,
  });
} finally {
  restore();
}
```

Create the output directory first. Method paths cannot contain `__proto__`, `constructor` or
`prototype`; instrument client instances, not prototype objects. Provider-specific Node examples,
streaming and pricing options are in the [TypeScript guide](typescript.md).

## Local and open-source inference

An OpenAI-compatible endpoint can be an existing Ollama/vLLM server or a private gateway. Supply the
endpoint's supported model identifier and its actual authentication requirements. Refract does not
start an inference server, download weights or translate unsupported APIs.

From a source checkout with an existing local endpoint:

```bash
uv run python examples/python/providers/openai_compatible.py \
  --base-url http://127.0.0.1:11434/v1 --provider ollama --model YOUR_INSTALLED_MODEL
uv run python examples/python/providers/custom.py
uv run python examples/python/providers/transformers_local.py --model-path /path/to/existing/weights
node examples/typescript/providers/local.mjs --help
```

The Transformers example requires application-installed `transformers` and its inference backend;
it loads existing weights with downloads disabled. The offline custom example requires no provider.

## LangChain and Langfuse

Construct a LangChain callback inside an active recording:

```python
import refract
from langchain_core.runnables import RunnableLambda
from refract.integrations.langchain import langchain_handler

chain = RunnableLambda(lambda query: {"answer": query.upper()})
with refract.run("chain") as recording:
    chain.invoke("hello", config={"callbacks": [langchain_handler(provider="custom")]})
```

Use either LangChain model callbacks or provider instrumentation for the same call. Enabling both
currently records duplicate generation events and can double-count usage. Callback errors are
available on `handler.errors`; LangChain is an optional dependency. This integration does not
implicitly execute or approve tools during replay.

Export completed runs to Langfuse with the [OpenTelemetry guide](otel.md#langfuse). The Python and
Node bridges map model, usage, cost and event type to Langfuse observations. They work with cloud
or self-hosted URLs; choose your project's region and keep keys in application configuration.

## Local versus production recording

Offline mode needs only an output path and the CLI. Local service mode runs the Inspector/API with
`docker compose up --build -d --wait`. Shared deployments use HTTPS, scoped API keys, encrypted
storage and an application exporter. These settings are independent of provider authentication.
See [production operation](../production.md) and SDK [Python](python.md)/[Node](typescript.md) guides
for bounded queues, retry spools and shutdown.

Usage comes from provider responses; prices are explicit configuration, not a live billing feed.
Missing usage/pricing is unknown, not zero. Tool-call proposals are distinct from actual tool
execution. See [metrics](metrics.md) for completeness and cost interpretation.

## Verification and examples

Python contracts exercise real installed SDKs against intercepted HTTP fixtures for OpenAI, Anthropic,
Azure, Google/Vertex and Bedrock. Node contracts exercise supported client method/stream shapes.
Custom adapters, errors, cancellation, usage and restoration have local tests. These do not make paid
provider calls or establish credential/region compatibility for your deployment.

Run `make setup && make test` for the locked test environment. Executable examples live in
[Python providers](../../examples/python/providers/custom.py),
[Node providers](../../examples/typescript/providers/README.md), and the
[example catalog](../../examples/README.md). Validate your chosen live model before deployment.
