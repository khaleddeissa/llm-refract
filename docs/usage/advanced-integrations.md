# Provider extensions and framework continuation

These interfaces are available in the 0.1.5 source checkout. Contract tests use local fixtures and
mock transports; they do not certify access to a particular cloud subscription or model deployment.

## Foundry and Azure

An Azure OpenAI client covers Azure OpenAI endpoints. Foundry's `/openai/v1/` endpoint also supports
OpenAI-compatible clients, including compatible non-OpenAI deployments. Configure the endpoint,
credentials and deployment name on the provider client; Refract observes the calls without changing
authentication. API availability still depends on the deployed model.

```python
from openai import OpenAI
import refract

client = OpenAI(
    base_url="https://YOUR-RESOURCE.services.ai.azure.com/openai/v1/", api_key=YOUR_TOKEN_PROVIDER
)
handle = refract.instrument_azure(client)
# Use the deployment name as model. AsyncOpenAI requires an async token provider.
```

Tests cover synchronous/asynchronous clients, the exact v1 route, API keys and rotating token callbacks.
See Microsoft's [endpoint guide](https://learn.microsoft.com/en-us/azure/foundry/foundry-models/concepts/endpoints).

## Native Bedrock, legacy Vertex and local inference

```python
handle = refract.instrument_bedrock_native(existing_boto_or_async_client)
handle = refract.instrument_vertex(existing_vertex_generative_model)
handle = refract.instrument_library(existing_ollama_client, "ollama")
handle = refract.instrument_library(existing_inference_client, "huggingface")
handle = refract.instrument_library(existing_llama_cpp_client, "llama_cpp")
handle = refract.instrument_library(existing_litellm_module, "litellm")
```

Native Bedrock observes `invoke_model` and `invoke_model_with_response_stream`; Converse retains its
existing adapter. Body observation is lazy: `read`, `readinto`, iteration and `iter_chunks` consume
only what the application requests. Close early and the recording identifies an incomplete response.
Usage normalization covers Anthropic, Titan, Llama and Bedrock invocation metrics when supplied.
File-like request bodies are not drained. Capture is bounded to 1 MiB per generation.

Library adapters cover the named clients' standard generation methods. Different inference APIs can
use `instrument_custom` with request/response normalizers. Credentials remain owned by the application.
Use `handle.uninstrument()` to restore methods. Never assume that an unobserved call has zero usage.

## Realtime

```python
handle = refract.instrument_realtime(connection, provider="openai", model="your-model")
```

Install after connecting, before receiving messages. It observes `recv()` and SDK iteration that
calls `recv()`. Each `response.created`/`response.done` pair becomes one generation. Text deltas and
reported usage are captured; binary audio is excluded. Closing a connection marks pending responses
incomplete. Up to 32 simultaneous responses are tracked. The application configures WebSocket
transport, authentication, microphone/audio handling and session settings.

This follows the [Realtime event lifecycle](https://developers.openai.com/api/docs/guides/realtime-conversations).

## LangChain and LangGraph

Inline LangChain callbacks share the active generation with provider instrumentation, preserving one
usage measurement for the observed call. Callbacks still record actual tool executions independently
of a provider's tool proposals. Keep callbacks and instrumented calls in the same execution context;
remote callbacks cannot provide an implicit correlation identifier.

```python
from refract.integrations.langgraph import LangGraphRuntime

runtime = LangGraphRuntime(compiled_graph, name="support-workflow-v1")
with refract.run("original"):
    checkpoint = runtime.checkpoint({"configurable": {"thread_id": "ticket-123"}})
branch = runtime.resume(
    checkpoint,
    thread_id="ticket-123",
    allow_live=True,
    updates={"candidate_model": "local-candidate"},
)
# Async graphs: await runtime.resume_async(...)
```

Install `llm-refract[langgraph]`. The compiled graph must have a checkpointer. Production continuations
need that application's persistent checkpoint store and compatible graph code. `thread_id` is supplied
separately by trusted application code and must match the checkpoint; authorize access to that thread
before calling this API. `allow_live=True` authorizes the configured graph, including its side effects.
`updates` uses framework reducers; `resume_value` answers an interrupt. The resulting recording contains
lineage and actual subsequent callback events. It does not restore arbitrary process memory or import
code from an artifact. Original checkpoint data remains unchanged.

## Node adapters

```typescript
import {
  refract,
  instrumentBedrockNative,
  instrumentLibrary,
  instrumentRealtime,
} from "@llm-refract/sdk";
const restoreBedrock = instrumentBedrockNative(bedrockRuntimeClient);
const restoreLocal = instrumentLibrary(ollamaClient, "ollama");
await refract.run("voice-session", async () => {
  const stop = instrumentRealtime(realtimeConnection, {
    provider: "openai",
    model: "your-model",
  });
  try {
    await yourConversation();
  } finally {
    stop();
  }
});
restoreBedrock();
restoreLocal();
```

Node supports AWS SDK v3 `InvokeModelCommand`/`InvokeModelWithResponseStreamCommand`, Google Gen AI
configured for Vertex, and Ollama/Hugging Face/llama-cpp-shaped clients. Realtime observes the SDK's
`event` emitter, binds events to the installation run even when the socket was created elsewhere,
and returns a cleanup function. Call cleanup before the run finishes to mark partial responses.
Text capture is bounded; raw binary audio is excluded. Different event transports use custom adapters.

## LiteLLM is an optional application integration

Refract's Python adapter already supports LiteLLM's `completion`, `acompletion`, `responses` and
`aresponses` methods when present. Install LiteLLM in the application that needs its provider routing;
the Refract storage/replay/comparison engine does not require it. Your application continues to own
LiteLLM routing, fallback, credentials and endpoint configuration.

```python
import litellm
import refract

handle = refract.instrument_library(litellm, "litellm")
try:
    with refract.run("routed-model-call"):
        # Explicit provider call: use your application's configured model and credentials.
        result = litellm.completion(
            model=configured_model,
            messages=[{"role": "user", "content": "Hello"}],
        )
finally:
    handle.uninstrument()
```

Choose one instrumentation layer for a LiteLLM call: wrap LiteLLM itself or the underlying provider.
LangChain/provider deduplication does not imply general LiteLLM/provider deduplication. LiteLLM's
embedding APIs can generate vectors in application code; submit them through the documented
[vector search API](search.md#vector-search-and-embedding-model-selection). The named instrumentation
adapter currently records the listed generation methods, not embedding calls or router administration.

For Node applications using a LiteLLM proxy's OpenAI-compatible endpoint, configure the OpenAI client
with that proxy URL and use `instrumentOpenAI(client)`. Proxy authentication and routing remain in your
application configuration. This does not require a LiteLLM dependency inside the Refract engine.

## Node LangChain and LangGraph callbacks

```typescript
import {
  refract,
  langchainHandler,
  instrumentLangChainModel,
} from "@llm-refract/sdk";

const restore = instrumentLangChainModel(chatModel); // your configured LangChain chat/LLM model
try {
  await refract.run("framework-request", async () => {
    const callbacks = [langchainHandler()];
    await chain.invoke({ question: "Hello" }, { callbacks });
  });
} finally {
  restore();
}
```

`langchainHandler()` records chain, model, tool and retriever callbacks, their causal parents, failures,
reported usage, and time to first token. It can be passed to LangGraph JS runnables using the same
callback contract. `instrumentLangChainModel(model)` observes that model's `invoke` and `stream`
methods and coordinates with these callbacks and nested provider wrappers to keep one generation
measurement. Instrument the model instances used by your chain at startup. The callback handler alone
is also useful when provider instrumentation is disabled. Existing application callbacks are retained;
pass them alongside Refract's callback in the configuration.

The base Node SDK does not import LangChain at runtime; applications install their own framework
version. Tests use the installed `@langchain/core` package with local runnables and fake chat models,
including streamed/nonstreamed model calls and a model delegating to an instrumented provider client.
Framework callbacks in another process require explicit distributed trace propagation; no local
wrapper can infer that relationship without a shared identifier.
