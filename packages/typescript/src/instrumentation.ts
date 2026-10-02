import { AsyncLocalStorage } from "node:async_hooks";
import { startSpan, type Json } from "./index.js";

const providerGeneration = new AsyncLocalStorage<
  NonNullable<ReturnType<typeof startSpan>>
>();
/** Internal coordination with framework callbacks; does not suppress independent calls. */
export function generationObserved(parentId?: string): boolean {
  const capture = providerGeneration.getStore();
  if (!capture?.isCurrentGeneration()) return false;
  if (parentId) capture.setParent(parentId);
  return true;
}

export interface InstrumentOptions {
  /** Explicit application-maintained USD rates per million tokens; no assumed pricing. */
  pricing?: Record<
    string,
    { input: number; output: number; cachedInput?: number }
  >;
  maxOutputChars?: number;
}
/** Numeric usage from a custom backend. Values must be nonnegative integer counts. */
export interface GenerationUsage {
  input_tokens?: number;
  output_tokens?: number;
  total_tokens?: number;
  cache_read_input_tokens?: number;
  cache_creation_input_tokens?: number;
}
export interface NormalizedGeneration {
  model?: string;
  output?: Json;
  /** A text delta for a streaming chunk. */
  text?: string;
  toolCalls?: Json[];
  usage?: GenerationUsage;
  failed?: boolean;
}
export interface CustomInstrumentOptions extends InstrumentOptions {
  provider: string;
  /** Method paths on an existing client, e.g. [["generate"]]. */
  methods: string[][];
  request?: (args: unknown[]) => { model?: string; input?: Json };
  /** Runs for a final response or each stream chunk; never changes application data. */
  normalize?: (
    value: unknown,
    context: { streaming: boolean },
  ) => NormalizedGeneration;
  /** Optional response property containing an async iterable, e.g. Bedrock's "stream". */
  streamKey?: string;
}
type Data = Record<string, unknown>;
type Adapter = Pick<
  CustomInstrumentOptions,
  "request" | "normalize" | "streamKey"
> & {
  matches?: (args: unknown[]) => boolean;
  callback?: boolean;
};
function object(value: unknown): Data {
  return value !== null && typeof value === "object" ? (value as Data) : {};
}
function json(value: unknown): Json {
  try {
    return JSON.parse(JSON.stringify(value ?? null)) as Json;
  } catch {
    return "[Unserializable]";
  }
}
function usage(
  value: unknown,
  provider: string,
  model: string,
  options: InstrumentOptions,
): Record<string, Json> {
  const raw = object(value);
  const count = (value: unknown): number | undefined =>
    typeof value === "number" && Number.isSafeInteger(value) && value >= 0
      ? value
      : undefined;
  const input = count(
    raw.input_tokens ??
      raw.prompt_tokens ??
      raw.promptTokenCount ??
      raw.inputTokens,
  );
  const candidate = count(raw.candidatesTokenCount);
  const output = count(
    raw.output_tokens ??
      raw.completion_tokens ??
      raw.outputTokens ??
      (candidate === undefined
        ? undefined
        : candidate + (count(raw.thoughtsTokenCount) ?? 0)),
  );
  const cached = count(
    raw.cache_read_input_tokens ??
      raw.cachedContentTokenCount ??
      raw.cacheReadInputTokens ??
      object(raw.input_tokens_details).cached_tokens ??
      object(raw.prompt_tokens_details).cached_tokens,
  );
  const created = count(
    raw.cache_creation_input_tokens ?? raw.cacheWriteInputTokens,
  );
  const attributes: Record<string, Json> = { provider, model };
  if (input !== undefined) attributes.input_tokens = input;
  if (output !== undefined) attributes.output_tokens = output;
  if (cached !== undefined) {
    attributes.cache_read_tokens = cached;
    attributes.cache_hit = cached > 0;
  }
  if (created !== undefined) attributes.cache_write_tokens = created;
  const inclusive = !["anthropic", "bedrock"].includes(provider);
  const measuredTotal = count(
    raw.total_tokens ?? raw.totalTokenCount ?? raw.totalTokens,
  );
  if (measuredTotal !== undefined) attributes.total_tokens = measuredTotal;
  else if (input !== undefined && output !== undefined)
    attributes.total_tokens =
      input + output + (inclusive ? 0 : (cached ?? 0) + (created ?? 0));
  const rate = options.pricing?.[model];
  if (rate && input !== undefined && output !== undefined) {
    const regular = inclusive
      ? Math.max(0, input - (cached ?? 0))
      : input + (created ?? 0);
    attributes.cost_usd =
      (regular * rate.input +
        (cached ?? 0) * (rate.cachedInput ?? rate.input) +
        output * rate.output) /
      1_000_000;
    attributes.cost_estimated = true;
  }
  return attributes;
}
function normalizeDefault(
  value: unknown,
  streaming: boolean,
): NormalizedGeneration {
  const data = object(value);
  const delta = object(data.delta);
  const choices = Array.isArray(data.choices) ? data.choices : [];
  const choiceDelta = object(object(choices[0]).delta);
  const toolCalls: Json[] = [];
  if (choiceDelta.tool_calls) toolCalls.push(json(choiceDelta.tool_calls));
  if (
    data.type === "content_block_start" &&
    object(data.content_block).type === "tool_use"
  )
    toolCalls.push(json(data.content_block));
  if (
    data.type === "response.output_item.done" &&
    object(data.item).type === "function_call"
  )
    toolCalls.push(json(data.item));
  return {
    model: typeof data.model === "string" ? data.model : undefined,
    usage: {
      ...object(object(data.message).usage),
      ...object(data.usage),
      ...object(object(data.response).usage),
    },
    text: streaming
      ? typeof data.delta === "string"
        ? data.delta
        : String(delta.text ?? choiceDelta.content ?? "")
      : undefined,
    toolCalls,
    failed:
      data.type === "error" ||
      data.type === "response.failed" ||
      data.status === "failed",
  };
}

function instrument(
  client: object,
  provider: string,
  paths: string[][],
  options: InstrumentOptions,
  adapter: Adapter = {},
): () => void {
  if (
    options.maxOutputChars !== undefined &&
    (!Number.isInteger(options.maxOutputChars) || options.maxOutputChars < 1)
  )
    throw new Error("maxOutputChars must be a positive integer");
  for (const rate of Object.values(options.pricing ?? {}))
    for (const value of Object.values(rate))
      if (!Number.isFinite(value) || value < 0)
        throw new Error("Pricing rates must be finite and nonnegative");
  // Validate every path before patching anything, including later paths in a batch.
  for (const path of paths)
    if (
      !path.length ||
      path.some(
        (key) =>
          typeof key !== "string" ||
          !key ||
          key === "__proto__" ||
          key === "prototype" ||
          key === "constructor",
      )
    )
      throw new Error("Unsafe instrumentation property path");
  const targets = paths.flatMap((path) => {
    let target = client as Data;
    for (const segment of path.slice(0, -1)) {
      // Nested namespaces must belong to this client, not an inherited prototype.
      if (!Object.prototype.hasOwnProperty.call(target, segment)) return [];
      target = object(target[segment]);
    }
    const constructor = Object.getOwnPropertyDescriptor(
      target,
      "constructor",
    )?.value;
    if (typeof constructor === "function" && constructor.prototype === target)
      throw new Error("Cannot instrument a prototype object");
    const key = path.at(-1)!;
    const original = target[key];
    if (typeof original !== "function") return [];
    const descriptor = Object.getOwnPropertyDescriptor(target, key);
    if (
      (descriptor && (!("value" in descriptor) || !descriptor.writable)) ||
      (!descriptor && !Object.isExtensible(target))
    )
      throw new Error("Instrumentation requires a writable client method");
    return [{ path, target, key, original, descriptor }];
  });
  const restores: (() => void)[] = [];
  for (const { path, target, key, original, descriptor } of targets) {
    const replacement = function (this: unknown, ...args: unknown[]) {
      if (providerGeneration.getStore()?.isCurrentGeneration())
        return Reflect.apply(original, this, args) as unknown;
      const params = object(args[0]);
      let request: { model?: string; input?: Json } = {};
      let matches = true;
      try {
        matches = adapter.matches?.(args) ?? true;
        if (matches)
          request = adapter.request?.(args) ?? {
            model: String(params.model ?? "unknown"),
            input: json(params),
          };
      } catch {
        matches = false;
      }
      if (!matches) return Reflect.apply(original, this, args) as unknown;
      const model = request.model ?? "unknown";
      let capture: ReturnType<typeof startSpan>;
      // Instrumentation failures must never prevent the provider call.
      try {
        capture = startSpan({
          type: "generation",
          name: `${provider}.${path.join(".")}`,
          input: request.input ?? null,
          attributes: { provider, model },
        });
      } catch {
        /* fail open */
      }
      const started = performance.now();
      const withinGeneration = <T>(fn: () => T): T =>
        capture
          ? providerGeneration.run(capture, () => capture.within(fn))
          : fn();
      const finish = (
        output: unknown,
        attributes: Record<string, Json>,
        failed = false,
      ) => {
        try {
          capture?.finish(json(output), attributes, failed);
        } catch {
          /* fail open */
        }
      };
      const failure = (error: unknown): never => {
        finish(
          null,
          { error_type: error instanceof Error ? error.name : "Error" },
          true,
        );
        throw error;
      };
      const capturedResults = new WeakMap<object, unknown>();
      const complete = (result: unknown): unknown => {
        if (result && typeof result === "object" && capturedResults.has(result))
          return capturedResults.get(result);
        let capturedResult: unknown;
        try {
          capturedResult = captureResult(result);
        } catch {
          // Provider results must survive unsupported or custom response shapes.
          finish(null, { provider, model, capture_incomplete: true });
          capturedResult = result;
        }
        if (result && typeof result === "object")
          capturedResults.set(result, capturedResult);
        return capturedResult;
      };
      const captureResult = (result: unknown): unknown => {
        const streamValue = adapter.streamKey
          ? object(result)[adapter.streamKey]
          : result;
        if (
          streamValue &&
          typeof object(streamValue)[
            Symbol.asyncIterator as unknown as string
          ] === "function"
        ) {
          const stream = streamValue as AsyncIterable<unknown>;
          let consumed = false;
          const wrappedStream = new Proxy(stream, {
            get(target, property) {
              if (property !== Symbol.asyncIterator) {
                const value = Reflect.get(target, property, target);
                return typeof value === "function" ? value.bind(target) : value;
              }
              return async function* () {
                if (consumed)
                  throw new Error("Provider stream already consumed");
                consumed = true;
                let text = "";
                let truncated = false;
                let first: number | undefined;
                let rawUsage: Data = {};
                let completed = false;
                let failed = false;
                let providerFailed = false;
                let incomplete = false;
                const tools: Json[] = [];
                try {
                  const iterator = withinGeneration(() =>
                    stream[Symbol.asyncIterator](),
                  );
                  const scopedStream = {
                    [Symbol.asyncIterator]() {
                      return {
                        next: () => withinGeneration(() => iterator.next()),
                        return: async () =>
                          withinGeneration(
                            () =>
                              iterator.return?.() ??
                              Promise.resolve({
                                done: true as const,
                                value: undefined,
                              }),
                          ),
                      };
                    },
                  };
                  for await (const chunk of scopedStream) {
                    try {
                      const normalized =
                        adapter.normalize?.(chunk, { streaming: true }) ??
                        normalizeDefault(chunk, true);
                      providerFailed ||= normalized.failed === true;
                      const token = normalized.text ?? "";
                      if (token && first === undefined)
                        first = performance.now() - started;
                      const limit = options.maxOutputChars ?? 1_000_000;
                      truncated ||= text.length + token.length > limit;
                      text += token.slice(0, Math.max(0, limit - text.length));
                      rawUsage = {
                        ...rawUsage,
                        ...Object.fromEntries(
                          Object.entries(normalized.usage ?? {}).filter(
                            ([, value]) => value !== undefined,
                          ),
                        ),
                      };
                      tools.push(...(normalized.toolCalls ?? []));
                    } catch {
                      incomplete = true; // Observing an unfamiliar chunk cannot break provider streams.
                    }
                    yield chunk;
                  }
                  completed = true;
                } catch (error) {
                  failed = true;
                  finish(
                    { text, tool_calls: tools },
                    {
                      ...usage(rawUsage, provider, model, options),
                      error_type: error instanceof Error ? error.name : "Error",
                      stream_completed: false,
                      ...(first === undefined ? {} : { ttft_ms: first }),
                    },
                    true,
                  );
                  throw error;
                } finally {
                  if (!failed)
                    finish(
                      { text, tool_calls: tools },
                      {
                        ...usage(rawUsage, provider, model, options),
                        stream_completed: completed && !providerFailed,
                        output_truncated: truncated,
                        ...(incomplete ? { capture_incomplete: true } : {}),
                        ...(first === undefined ? {} : { ttft_ms: first }),
                      },
                      providerFailed,
                    );
                }
              };
            },
          });
          if (!adapter.streamKey) return wrappedStream;
          return new Proxy(result as object, {
            get(target, property) {
              if (property === adapter.streamKey) return wrappedStream;
              const value = Reflect.get(target, property, target);
              return typeof value === "function" ? value.bind(target) : value;
            },
          });
        }
        const normalized =
          adapter.normalize?.(result, { streaming: false }) ??
          normalizeDefault(result, false);
        finish(
          normalized.output === undefined ? result : normalized.output,
          usage(normalized.usage, provider, normalized.model ?? model, options),
          normalized.failed,
        );
        return result;
      };
      try {
        const callbackIndex = adapter.callback
          ? args.findIndex((arg) => typeof arg === "function")
          : -1;
        if (callbackIndex >= 0) {
          const callerContext = AsyncLocalStorage.snapshot();
          const callback = args[callbackIndex] as (
            ...values: unknown[]
          ) => unknown;
          args[callbackIndex] = function (
            this: unknown,
            error: unknown,
            value: unknown,
            ...rest: unknown[]
          ) {
            if (error)
              finish(
                null,
                { error_type: error instanceof Error ? error.name : "Error" },
                true,
              );
            else value = complete(value);
            return callerContext(() =>
              Reflect.apply(callback, this, [error, value, ...rest]),
            );
          };
        }
        const invoke = () => Reflect.apply(original, this, args) as unknown;
        const result = withinGeneration(invoke);
        if (callbackIndex >= 0) return result;
        if (!capture) return result;
        if (!result || typeof object(result).then !== "function")
          return complete(result);
        // SDK promise helpers carry response metadata and must remain callable.
        // Observe lazily: eagerly awaiting asResponse() would consume a raw body.
        let observed: Promise<unknown> | undefined;
        const observe = () =>
          (observed ??= Promise.resolve(result).then(complete, failure));
        return new Proxy(result as object, {
          get(target, property) {
            if (
              property === "then" ||
              property === "catch" ||
              property === "finally"
            ) {
              const promise = observe();
              return promise[property].bind(promise);
            }
            const value = Reflect.get(target, property, target);
            if (property === "withResponse" && typeof value === "function")
              return (...helperArgs: unknown[]) =>
                Promise.resolve(Reflect.apply(value, target, helperArgs)).then(
                  (response) => ({
                    ...object(response),
                    data: complete(object(response).data),
                  }),
                  failure,
                );
            if (property === "asResponse" && typeof value === "function")
              return (...helperArgs: unknown[]) =>
                Promise.resolve(Reflect.apply(value, target, helperArgs)).then(
                  (response) => {
                    finish(null, {
                      provider,
                      model,
                      response_body_captured: false,
                    });
                    return response;
                  },
                  failure,
                );
            return typeof value === "function" ? value.bind(target) : value;
          },
        });
      } catch (error) {
        return failure(error);
      }
    };
    Object.defineProperty(target, key, {
      ...(descriptor ?? {
        configurable: true,
        enumerable: true,
        writable: true,
      }),
      value: replacement,
    });
    restores.push(() => {
      if (Object.getOwnPropertyDescriptor(target, key)?.value !== replacement)
        return;
      if (descriptor) Object.defineProperty(target, key, descriptor);
      else Reflect.deleteProperty(target, key);
    });
  }
  if (!restores.length)
    throw new Error(`No supported ${provider} methods found on client`);
  return () => restores.reverse().forEach((restore) => restore());
}
/** Patch an existing OpenAI-compatible client; returns a function restoring its methods. */
export function instrumentOpenAI(
  client: object,
  options: InstrumentOptions = {},
): () => void {
  return instrument(
    client,
    "openai",
    [
      ["responses", "create"],
      ["chat", "completions", "create"],
    ],
    options,
  );
}
/** Patch Anthropic messages.create; consume streams inside refract.run. */
export function instrumentAnthropic(
  client: object,
  options: InstrumentOptions = {},
): () => void {
  return instrument(client, "anthropic", [["messages", "create"]], options);
}

/** Instrument AzureOpenAI clients configured by the application. */
export function instrumentAzureOpenAI(
  client: object,
  options: InstrumentOptions = {},
): () => void {
  return instrument(
    client,
    "azure",
    [
      ["responses", "create"],
      ["chat", "completions", "create"],
    ],
    options,
  );
}
function normalizeGoogle(value: unknown): NormalizedGeneration {
  const data = object(value);
  const candidates = Array.isArray(data.candidates) ? data.candidates : [];
  const parts = candidates
    .flatMap((candidate) => {
      const content = object(object(candidate).content);
      return Array.isArray(content.parts) ? content.parts : [];
    })
    .map(object);
  return {
    model:
      typeof data.modelVersion === "string" ? data.modelVersion : undefined,
    text: parts
      .filter((part) => !part.thought)
      .map((part) => (typeof part.text === "string" ? part.text : ""))
      .join(""),
    toolCalls: parts
      .filter((part) => part.functionCall)
      .map((part) => json(part.functionCall)),
    usage: object(data.usageMetadata),
    failed:
      Boolean(object(data.promptFeedback).blockReason) ||
      candidates.some((candidate) =>
        [
          "SAFETY",
          "RECITATION",
          "BLOCKLIST",
          "PROHIBITED_CONTENT",
          "SPII",
          "MALFORMED_FUNCTION_CALL",
        ].includes(String(object(candidate).finishReason)),
      ),
  };
}
/** Google Gen AI SDK: models.generateContent and models.generateContentStream. */
export function instrumentGemini(
  client: object,
  options: InstrumentOptions = {},
): () => void {
  return instrument(
    client,
    "gemini",
    [
      ["models", "generateContent"],
      ["models", "generateContentStream"],
    ],
    options,
    { normalize: normalizeGoogle },
  );
}
/** Google Gen AI configured with vertexai: true; credentials stay on the original client. */
export function instrumentVertex(
  client: object,
  options: InstrumentOptions = {},
): () => void {
  return instrument(
    client,
    "vertex",
    [
      ["models", "generateContent"],
      ["models", "generateContentStream"],
    ],
    options,
    { normalize: normalizeGoogle },
  );
}
/** AWS SDK v3 BedrockRuntimeClient.send: ConverseCommand / ConverseStreamCommand only. */
export function instrumentBedrock(
  client: object,
  options: InstrumentOptions = {},
): () => void {
  return instrument(client, "bedrock", [["send"]], options, {
    callback: true,
    matches: (args) =>
      ["ConverseCommand", "ConverseStreamCommand"].includes(
        String(
          object(args[0]).constructor && (args[0] as object).constructor.name,
        ),
      ),
    request: (args) => {
      const input = object(object(args[0]).input);
      return { model: String(input.modelId ?? "unknown"), input: json(input) };
    },
    streamKey: "stream",
    normalize: (value) => {
      const data = object(value);
      const delta = object(object(data.contentBlockDelta).delta);
      const start = object(object(data.contentBlockStart).start);
      return {
        usage: {
          ...object(data.usage),
          ...object(object(data.metadata).usage),
        },
        text: typeof delta.text === "string" ? delta.text : "",
        toolCalls: [start.toolUse, delta.toolUse].filter(Boolean).map(json),
        failed: Object.keys(data).some((key) => key.endsWith("Exception")),
      };
    },
  });
}
/** Adapt any synchronous/Promise or async-iterable model client to canonical generation events. */
export function instrumentCustom(
  client: object,
  options: CustomInstrumentOptions,
): () => void {
  if (!options.provider.trim()) throw new Error("provider must not be empty");
  if (
    !options.methods.length ||
    options.methods.some((path) => !path.length || path.some((key) => !key))
  )
    throw new Error("methods must contain nonempty property paths");
  return instrument(
    client,
    options.provider,
    options.methods,
    options,
    options,
  );
}

/** Observe a LangChain chat/LLM model's invoke and stream methods, including nested SDK calls. */
export function instrumentLangChainModel(
  client: object,
  options: InstrumentOptions = {},
): () => void {
  if (typeof object(client)._llmType !== "function")
    throw new Error(
      "instrumentLangChainModel requires a LangChain language model",
    );
  return instrument(client, "langchain", [["invoke"], ["stream"]], options, {
    request: (args) => ({
      model: String(
        object(client).model ?? object(client).modelName ?? "langchain-model",
      ),
      input: json(args[0]),
    }),
    normalize: (value) => {
      const message = object(value);
      return {
        output: json({
          content: message.content ?? value,
          tool_calls: message.tool_calls ?? [],
          usage_metadata: message.usage_metadata ?? {},
        }),
        text: typeof message.content === "string" ? message.content : "",
        usage: object(message.usage_metadata),
      };
    },
  });
}

/** Native AWS SDK v3 InvokeModel/InvokeModelWithResponseStream without eager stream reads. */
export function instrumentBedrockNative(
  client: object,
  options: InstrumentOptions = {},
): () => void {
  const decode = (body: unknown): Data => {
    if (typeof body === "string") return object(JSON.parse(body));
    if (body instanceof Uint8Array)
      return object(JSON.parse(new TextDecoder().decode(body)));
    return object(body);
  };
  return instrument(client, "bedrock", [["send"]], options, {
    callback: true,
    matches: (args) =>
      ["InvokeModelCommand", "InvokeModelWithResponseStreamCommand"].includes(
        args[0]?.constructor.name ?? "",
      ),
    streamKey: "body",
    request: (args) => {
      const input = object(object(args[0]).input);
      return {
        model: String(input.modelId ?? "unknown"),
        input: json(decode(input.body)),
      };
    },
    normalize: (value, { streaming }) => {
      const raw = object(value);
      const data = decode(streaming ? object(raw.chunk).bytes : raw.body);
      const metrics = object(data["amazon-bedrock-invocationMetrics"]);
      const normal = normalizeDefault(data, streaming);
      const titan = object(
        Array.isArray(data.results) ? data.results[0] : undefined,
      );
      return {
        ...normal,
        output: json(data),
        text: String(
          normal.text ||
            data.generation ||
            data.outputText ||
            titan.outputText ||
            "",
        ),
        usage: {
          ...normal.usage,
          input_tokens: (data.inputTokenCount ??
            data.prompt_token_count ??
            metrics.inputTokenCount ??
            normal.usage?.input_tokens) as number | undefined,
          output_tokens: (data.generation_token_count ??
            titan.tokenCount ??
            data.totalOutputTextTokenCount ??
            metrics.outputTokenCount ??
            normal.usage?.output_tokens) as number | undefined,
        },
        failed: Object.keys(raw).some((key) => key.endsWith("Exception")),
      };
    },
  });
}

/** Named adapters for common local/custom inference libraries. */
export function instrumentLibrary(
  client: object,
  library: "ollama" | "huggingface" | "llama_cpp",
  options: InstrumentOptions = {},
): () => void {
  const methods = {
    ollama: [["chat"], ["generate"]],
    huggingface: [
      ["chatCompletion"],
      ["chatCompletionStream"],
      ["textGeneration"],
      ["textGenerationStream"],
    ],
    llama_cpp: [["createCompletion"], ["createChatCompletion"]],
  };
  return instrument(client, library, methods[library], options, {
    normalize: (value, context) => {
      const data = object(value);
      const normal = normalizeDefault(value, context.streaming);
      return library === "ollama"
        ? {
            ...normal,
            text: String(data.response ?? object(data.message).content ?? ""),
            usage: {
              input_tokens: data.prompt_eval_count as number | undefined,
              output_tokens: data.eval_count as number | undefined,
            },
          }
        : normal;
    },
  });
}

/** Observe an OpenAI-compatible Realtime event emitter inside an active refract.run. */
export function instrumentRealtime(
  connection: {
    on(event: "event", listener: (event: unknown) => void): unknown;
    off(event: "event", listener: (event: unknown) => void): unknown;
  },
  options: InstrumentOptions & { provider: string; model: string },
): () => void {
  const limit = options.maxOutputChars ?? 1_000_000;
  if (!Number.isInteger(limit) || limit < 1 || limit > 1_000_000)
    throw new Error("maxOutputChars must be between 1 and 1000000");
  if (!options.provider.trim() || !options.model.trim())
    throw new Error("provider and model are required");
  const within = AsyncLocalStorage.snapshot();
  const pending = new Map<
    string,
    {
      span: NonNullable<ReturnType<typeof startSpan>>;
      text: string;
      truncated: boolean;
      started: number;
      first?: number;
    }
  >();
  let stopped = false;
  const listener = (value: unknown) => {
    if (stopped) return;
    try {
      within(() => {
        const event = object(value);
        const response = object(event.response);
        const id = response.id ?? event.response_id;
        if (typeof id !== "string" || !id || id.length > 256) return;
        if (
          event.type === "response.created" &&
          !pending.has(id) &&
          pending.size < 32
        ) {
          const span = startSpan({
            type: "generation",
            name: `${options.provider}.realtime`,
            attributes: {
              provider: options.provider,
              model: options.model,
              response_id: id,
            },
          });
          if (span)
            pending.set(id, {
              span,
              text: "",
              truncated: false,
              started: performance.now(),
            });
        }
        const capture = pending.get(id);
        if (!capture) return;
        if (
          [
            "response.text.delta",
            "response.output_text.delta",
            "response.audio_transcript.delta",
            "response.output_audio_transcript.delta",
          ].includes(String(event.type)) &&
          typeof event.delta === "string"
        ) {
          if (event.delta && capture.first === undefined)
            capture.first = performance.now() - capture.started;
          capture.truncated ||=
            capture.text.length + event.delta.length > limit;
          capture.text += event.delta.slice(
            0,
            Math.max(0, limit - capture.text.length),
          );
        }
        if (event.type === "response.done") {
          capture.span.finish(
            { text: capture.text },
            {
              ...usage(
                response.usage,
                options.provider,
                options.model,
                options,
              ),
              stream_completed: response.status === "completed",
              output_truncated: capture.truncated,
              ...(capture.first === undefined
                ? {}
                : { ttft_ms: capture.first }),
            },
            response.status === "failed",
          );
          pending.delete(id);
        }
      });
    } catch {
      /* Observation must not interrupt the provider's event handlers. */
    }
  };
  connection.on("event", listener);
  return () => {
    if (stopped) return;
    stopped = true;
    connection.off("event", listener);
    for (const capture of pending.values())
      capture.span.finish(
        { text: capture.text },
        {
          stream_completed: false,
          capture_incomplete: true,
          output_truncated: capture.truncated,
        },
      );
    pending.clear();
  };
}
