import { startSpan, type Json } from "./index.js";

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
  const count = (value: unknown) =>
    typeof value === "number" && Number.isSafeInteger(value) && value >= 0
      ? value
      : 0;
  const input = count(
    raw.input_tokens ??
      raw.prompt_tokens ??
      raw.promptTokenCount ??
      raw.inputTokens,
  );
  const output = count(
    raw.output_tokens ??
      raw.completion_tokens ??
      raw.outputTokens ??
      count(raw.candidatesTokenCount) + count(raw.thoughtsTokenCount),
  );

  const cached = count(
    raw.cache_read_input_tokens ??
      raw.cachedContentTokenCount ??
      raw.cacheReadInputTokens ??
      object(raw.input_tokens_details).cached_tokens ??
      object(raw.prompt_tokens_details).cached_tokens ??
      0,
  );
  const created = count(
    raw.cache_creation_input_tokens ?? raw.cacheWriteInputTokens,
  );
  const attributes: Record<string, Json> = { provider, model };
  if (Object.keys(raw).length)
    Object.assign(attributes, {
      input_tokens: input,
      output_tokens: output,
      cache_read_tokens: cached,
      cache_write_tokens: created,
      total_tokens:
        raw.total_tokens !== undefined ||
        raw.totalTokenCount !== undefined ||
        raw.totalTokens !== undefined
          ? count(raw.total_tokens ?? raw.totalTokenCount ?? raw.totalTokens)
          : input + output + (provider === "anthropic" ? cached + created : 0),
      cache_hit: cached > 0,
    });
  const rate = options.pricing?.[model];
  if (rate && Object.keys(raw).length) {
    const regular =
      provider === "anthropic" ? input + created : Math.max(0, input - cached);
    attributes.cost_usd =
      (regular * rate.input +
        cached * (rate.cachedInput ?? rate.input) +
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
  const restores: (() => void)[] = [];
  for (const path of paths) {
    let target = client as Data;
    for (const segment of path.slice(0, -1)) target = object(target[segment]);
    const key = path.at(-1)!;
    const original = target[key];
    if (typeof original !== "function") continue;
    const replacement = function (this: unknown, ...args: unknown[]) {
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
                  for await (const chunk of stream) {
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
                      rawUsage = { ...rawUsage, ...normalized.usage };
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
        const invoke = () => Reflect.apply(original, this, args) as unknown;
        const result = capture ? capture.within(invoke) : invoke();
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
    target[key] = replacement;
    restores.push(() => {
      if (target[key] === replacement) target[key] = original;
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
    matches: (args) =>
      ["ConverseCommand", "ConverseStreamCommand"].includes(
        String(
          object(args[0]).constructor && (args[0] as object).constructor.name,
        ),
      ) && !args.some((arg) => typeof arg === "function"),
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
