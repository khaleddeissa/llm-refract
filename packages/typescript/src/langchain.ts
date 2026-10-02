import { startSpan, type EventInput, type Json } from "./index.js";
import { generationObserved } from "./instrumentation.js";

type Span = NonNullable<ReturnType<typeof startSpan>>;
function object(value: unknown): Record<string, unknown> {
  return value !== null && typeof value === "object"
    ? (value as Record<string, unknown>)
    : {};
}
function json(value: unknown): Json {
  try {
    return JSON.parse(JSON.stringify(value ?? null)) as Json;
  } catch {
    return "[Unserializable]";
  }
}

/** Pass as a LangChain/LangGraph callback. No framework dependency is needed for SDK imports. */
export function langchainHandler() {
  const spans = new Map<
    string,
    { span: Span; started: number; ttft?: number }
  >();
  const suppressed = new Set<string>();
  const errors: string[] = [];
  const error = (failure: unknown) => {
    errors.push(failure instanceof Error ? failure.name : "Error");
    if (errors.length > 100) errors.shift();
  };
  const start = (
    type: EventInput["type"],
    serialized: unknown,
    input: unknown,
    id: string,
    parent?: string,
    name?: string,
  ) => {
    try {
      if (
        type === "generation" &&
        generationObserved(parent ? spans.get(parent)?.span.id : undefined)
      ) {
        suppressed.add(id);
        return;
      }
      const original = object(serialized);
      const span = startSpan({
        type,
        name: name ?? String(original.name ?? type),
        input: json(input),
        parent_id: parent ? spans.get(parent)?.span.id : undefined,
        attributes: { framework: "langchain", framework_run_id: id },
        replay_policy: type === "tool.call" ? "REQUIRES_APPROVAL" : "RECORDED",
      });
      if (span) spans.set(id, { span, started: performance.now() });
    } catch (failure) {
      error(failure);
    }
  };
  const finish = (value: unknown, id: string, failed = false) => {
    if (suppressed.delete(id)) return;
    const captured = spans.get(id);
    if (!captured) return;
    spans.delete(id);
    try {
      const output = object(value);
      const generations = Array.isArray(output.generations)
        ? output.generations
        : [];
      const first = Array.isArray(generations[0])
        ? object(generations[0][0])
        : {};
      const metadata = object(object(first.message).usage_metadata);
      const tokenUsage = object(object(output.llmOutput).tokenUsage);
      const attrs: Record<string, Json> = {};
      const input = metadata.input_tokens ?? tokenUsage.promptTokens;
      const out = metadata.output_tokens ?? tokenUsage.completionTokens;
      for (const [key, count] of Object.entries({
        input_tokens: input,
        output_tokens: out,
      }))
        if (
          typeof count === "number" &&
          Number.isSafeInteger(count) &&
          count >= 0
        )
          attrs[key] = count;
      if (
        typeof attrs.input_tokens === "number" &&
        typeof attrs.output_tokens === "number"
      )
        attrs.total_tokens = attrs.input_tokens + attrs.output_tokens;
      if (captured.ttft !== undefined) attrs.ttft_ms = captured.ttft;
      if (failed)
        attrs.exception_type = value instanceof Error ? value.name : "Error";
      captured.span.finish(failed ? null : json(value), attrs, failed);
    } catch (failure) {
      error(failure);
      captured.span.finish(null, { capture_incomplete: true }, failed);
    }
  };
  const fail = (value: unknown, id: string) => finish(value, id, true);
  return {
    name: "refract",
    awaitHandlers: true,
    raiseError: false,
    errors,
    handleChainStart: (
      chain: unknown,
      inputs: unknown,
      id: string,
      parent?: string,
      _tags?: string[],
      _metadata?: Record<string, unknown>,
      _runType?: string,
      name?: string,
    ) => start("decision", chain, inputs, id, parent, name),
    handleChainEnd: (value: unknown, id: string) => finish(value, id),
    handleChainError: fail,
    handleLLMStart: (
      llm: unknown,
      prompts: unknown,
      id: string,
      parent?: string,
      _extra?: Record<string, unknown>,
      _tags?: string[],
      _metadata?: Record<string, unknown>,
      name?: string,
    ) => start("generation", llm, prompts, id, parent, name),
    handleChatModelStart: (
      llm: unknown,
      messages: unknown,
      id: string,
      parent?: string,
      _extra?: Record<string, unknown>,
      _tags?: string[],
      _metadata?: Record<string, unknown>,
      name?: string,
    ) => start("generation", llm, messages, id, parent, name),
    handleLLMEnd: (value: unknown, id: string) => finish(value, id),
    handleLLMError: fail,
    handleLLMNewToken: (token: string, _indices: unknown, id: string) => {
      const entry = spans.get(id);
      if (entry && token && entry.ttft === undefined)
        entry.ttft = performance.now() - entry.started;
    },
    handleToolStart: (
      tool: unknown,
      input: unknown,
      id: string,
      parent?: string,
      _tags?: string[],
      _metadata?: Record<string, unknown>,
      name?: string,
    ) => start("tool.call", tool, input, id, parent, name),
    handleToolEnd: (value: unknown, id: string) => finish(value, id),
    handleToolError: fail,
    handleRetrieverStart: (
      retriever: unknown,
      query: unknown,
      id: string,
      parent?: string,
      _tags?: string[],
      _metadata?: Record<string, unknown>,
      name?: string,
    ) => start("retrieval", retriever, query, id, parent, name),
    handleRetrieverEnd: (value: unknown, id: string) => finish(value, id),
    handleRetrieverError: fail,
  };
}
