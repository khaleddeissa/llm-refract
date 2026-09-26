# Provider contracts, offline

Build the SDK with `npm run build --workspace packages/typescript`, then run from the repository root:

```bash
node examples/typescript/providers/record.mjs
```

The script records seven generation events into `.examples/providers-<timestamp>.rfr`: OpenAI,
Azure OpenAI, Anthropic, Gemini, Vertex AI, Amazon Bedrock and a custom local backend. Every response
is a hardcoded fixture; no provider SDK, credential or paid inference call is needed. Input/output
counts are deliberately small (eight input and two output tokens; the custom backend reports only
two output tokens). Anthropic splits its input into six new and two cached tokens. Google and Bedrock
fixtures emit text followed by usage metadata, exercising the same async streaming contracts as
their supported real SDK methods. The measured timings come from consuming these local fixtures,
not cloud latency. These files demonstrate capture and interoperability, not model quality.

Replace the fixture clients with application-configured SDK clients to record real calls. See the
[TypeScript guide](../../../docs/usage/typescript.md) for real client configuration, adapter boundaries,
local output, authenticated export and custom normalization. For Bedrock, use the real AWS SDK's
`ConverseCommand` or `ConverseStreamCommand` classes; the example class is only an offline fixture.

## Your local model server

`local.mjs` makes real requests to an existing OpenAI-compatible endpoint and records streamed
responses. Install the `openai` SDK in your application, build Refract, then run from the repository:

```bash
# Ollama: use the exact name of a model you already pulled.
LLM_BASE_URL=http://localhost:11434/v1 LLM_MODEL=your-local-model node examples/typescript/providers/local.mjs
# vLLM: use the model identifier configured on your running server.
LLM_BASE_URL=http://localhost:8000/v1 LLM_MODEL=your-served-model node examples/typescript/providers/local.mjs
```

Set `LLM_API_KEY` if your inference endpoint requires authentication. The output is
`.examples/local-model-<timestamp>.rfr`; `REFRACT_ENDPOINT` and `REFRACT_API_KEY` additionally submit it
to the Refract service. Inference and recording are separate services: choose different ports if
both run locally. Set `LLM_INCLUDE_USAGE=1` only when your endpoint supports streaming usage.
The [Ollama](https://docs.ollama.com/api/openai-compatibility) and
[vLLM](https://docs.vllm.ai/en/latest/serving/openai_compatible_server/) compatibility guides describe
their supported request fields. This script does not download or serve model weights.

## Send a recording to Langfuse

`langfuse.mjs` exports an existing artifact using the SDK's Langfuse OTLP mapping. Set
`LANGFUSE_BASE_URL`, `LANGFUSE_PUBLIC_KEY`, and `LANGFUSE_SECRET_KEY` for your cloud or local instance:

```bash
node examples/typescript/providers/langfuse.mjs examples/artifacts/demo.rfr
```

The script sends data only when invoked. It checks ingestion failures and uses deterministic span IDs.
No paid model call is made. Credentials are HTTP headers and never recording attributes.
