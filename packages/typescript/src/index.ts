import { AsyncLocalStorage } from "node:async_hooks";
import { createHash, randomUUID } from "node:crypto";
import { writeFile } from "node:fs/promises";
export type Json =
  null | boolean | number | string | Json[] | { [key: string]: Json };
export type EventType =
  | "generation"
  | "tool.call"
  | "retrieval"
  | "decision"
  | "state.change"
  | "checkpoint"
  | "handoff"
  | "human"
  | "artifact"
  | "error";
export type ReplayPolicy =
  "READ_ONLY" | "MOCK" | "RECORDED" | "LIVE" | "REQUIRES_APPROVAL" | "BLOCKED";
export interface EventInput {
  type: EventType;
  name: string;
  input?: Json;
  output?: Json;
  parent_id?: string;
  duration_ms?: number;
  attributes?: Record<string, Json>;
  replay_policy?: ReplayPolicy;
  status?: "running" | "completed" | "failed";
}
export interface ExecutionEvent extends Omit<EventInput, "parent_id"> {
  id: string;
  run_id: string;
  parent_id: string | null;
  timestamp: string;
}
export interface Execution {
  spec_version: "refract.execution.v1";
  id: string;
  name: string;
  status: "running" | "completed" | "failed";
  started_at: string;
  ended_at: string | null;
  metadata: Record<string, Json>;
  events: ExecutionEvent[];
}
const metricKeys = new Set([
  "input_tokens",
  "output_tokens",
  "cache_read_tokens",
  "cache_write_tokens",
  "total_tokens",
  "prompt_tokens",
  "completion_tokens",
]);
export function redact(value: Json): Json {
  if (Array.isArray(value)) return value.map(redact);
  if (value !== null && typeof value === "object")
    return Object.fromEntries(
      Object.entries(value).map(([key, v]) => [
        key,
        !(
          metricKeys.has(key) &&
          typeof v === "number" &&
          Number.isSafeInteger(v) &&
          v >= 0
        ) &&
        /password|secret|token|api_key|authorization|cookie|email/.test(
          key.toLowerCase().replaceAll("-", "_"),
        )
          ? "[REDACTED]"
          : redact(v),
      ]),
    );
  if (typeof value === "number" && !Number.isFinite(value))
    throw new Error("non-finite JSON number");
  return value;
}
const context = new AsyncLocalStorage<{
  execution: Execution;
  active: boolean;
  sampled: boolean;
  parentId?: string;
}>();
export function event(input: EventInput): string {
  const state = context.getStore();
  if (!state?.active || state.execution.status !== "running")
    throw new Error("event requires an active refract.run");
  if (!input.name.trim() || (input.duration_ms ?? 0) < 0)
    throw new Error("invalid event name/duration");
  if (!state.sampled) return `evt_${randomUUID()}`;
  input = { ...input, parent_id: input.parent_id ?? state.parentId };
  if (
    input.parent_id &&
    !state.execution.events.some((e) => e.id === input.parent_id)
  )
    throw new Error("parent must precede child");
  const e = redact({
    ...input,
    id: `evt_${randomUUID()}`,
    run_id: state.execution.id,
    parent_id: input.parent_id ?? null,
    timestamp: new Date().toISOString(),
    status: input.status ?? "completed",
    duration_ms: input.duration_ms ?? 0,
    input: input.input ?? null,
    output: input.output ?? null,
    attributes: input.attributes ?? {},
    replay_policy: input.replay_policy ?? "RECORDED",
  } as Json) as unknown as ExecutionEvent;
  state.execution.events.push(e);
  return e.id;
}
/** Nested spans preserve causal parents across concurrent asynchronous work. */
export function startSpan(input: EventInput) {
  const state = context.getStore();
  if (!state?.active || state.execution.status !== "running" || !state.sampled)
    return undefined;
  const id = event({ ...input, status: "running" });
  const started = performance.now();
  const captured = state.execution.events.find((item) => item.id === id)!;
  return {
    id,
    within<T>(fn: () => T): T {
      return context.run({ ...state, parentId: id }, fn);
    },
    finish(
      output: Json,
      attributes: Record<string, Json> = {},
      failed = false,
    ) {
      if (
        !state.active ||
        state.execution.status !== "running" ||
        captured.status !== "running"
      )
        return;
      captured.duration_ms = performance.now() - started;
      captured.output = redact(output);
      captured.attributes = redact({
        ...captured.attributes,
        ...attributes,
      }) as Record<string, Json>;
      captured.status = failed ? "failed" : "completed";
    },
  };
}
export async function span<T>(
  input: EventInput,
  fn: () => T | Promise<T>,
): Promise<T> {
  const current = startSpan(input);
  try {
    const result = await (current ? current.within(fn) : fn());
    let output: Json = null;
    try {
      output = JSON.parse(JSON.stringify(result ?? null)) as Json;
    } catch {
      output = "[Unserializable]";
    }
    current?.finish(output);
    return result;
  } catch (error) {
    current?.finish(
      null,
      { error_type: error instanceof Error ? error.name : "Error" },
      true,
    );
    throw error;
  }
}
/** UTF-8 checksum header + formatted execution JSON. */
export function pack(execution: Execution): Uint8Array {
  const safe = redact(execution as unknown as Json) as unknown as Execution;
  const payload = Buffer.from(JSON.stringify(safe, null, 2) + "\n");
  if (payload.length > 16 * 1024 * 1024)
    throw new Error("artifact exceeds size limit");
  const header = {
    format: "refract.artifact.v1",
    encoding: "json",
    sha256: createHash("sha256").update(payload).digest("hex"),
  };
  return Buffer.concat([Buffer.from(JSON.stringify(header) + "\n"), payload]);
}
export function unpack(bytes: Uint8Array): Execution {
  const data = Buffer.from(bytes);
  if (data.length > 16 * 1024 * 1024 + 4097)
    throw new Error("artifact exceeds size limit");
  const split = data.indexOf(10);
  if (split < 0 || split > 4096) throw new Error("invalid artifact header");
  const header = JSON.parse(data.subarray(0, split).toString("utf8"));
  const payload = data.subarray(split + 1);
  if (payload.length > 16 * 1024 * 1024)
    throw new Error("payload exceeds size limit");
  if (header.format !== "refract.artifact.v1" || header.encoding !== "json")
    throw new Error("unsupported profile; use Rust CLI for legacy ZIP");
  if (createHash("sha256").update(payload).digest("hex") !== header.sha256)
    throw new Error("checksum mismatch");
  const run = JSON.parse(payload.toString("utf8"));
  if (
    run?.spec_version !== "refract.execution.v1" ||
    !Array.isArray(run.events)
  )
    throw new Error("invalid execution");
  return run as Execution;
}
export async function run<T>(
  name: string,
  fn: () => T | Promise<T>,
  options: {
    path?: string;
    endpoint?: string;
    metadata?: Record<string, Json>;
    apiKey?: string;
    sampleRate?: number;
    failOpen?: boolean;
    onError?: (error: unknown) => void;
    exporter?: { export(execution: Execution): void | Promise<void> };
    onComplete?: (execution: Execution) => void;
  } = {},
): Promise<T> {
  if (!name.trim()) throw new Error("run name cannot be empty");
  const sampleRate = options.sampleRate ?? 1;
  if (sampleRate < 0 || sampleRate > 1 || !Number.isFinite(sampleRate))
    throw new Error("sampleRate must be between 0 and 1");
  const execution: Execution = {
    spec_version: "refract.execution.v1",
    id: `run_${randomUUID()}`,
    name,
    status: "running",
    started_at: new Date().toISOString(),
    ended_at: null,
    metadata: redact(options.metadata ?? {}) as Record<string, Json>,
    events: [],
  };
  const state = {
    execution,
    active: true,
    sampled: sampleRate === 1 || Math.random() < sampleRate,
  };
  return context.run(state, async () => {
    let failed = false;
    try {
      const result = await fn();
      execution.status = "completed";
      return result;
    } catch (error) {
      failed = true;
      event({ type: "error", name: "Execution failed", status: "failed" });
      execution.status = "failed";
      throw error;
    } finally {
      state.active = false;
      execution.ended_at = new Date().toISOString();
      try {
        if (state.sampled) {
          if (options.path)
            await writeFile(
              options.path,
              options.path.endsWith(".rfr")
                ? pack(execution)
                : JSON.stringify(execution, null, 2),
              { flag: "wx" },
            );
          if (options.endpoint) {
            const response = await fetch(
              `${options.endpoint.replace(/\/$/, "")}/v1/runs`,
              {
                method: "POST",
                headers: {
                  "Content-Type": "application/json",
                  ...(options.apiKey
                    ? { Authorization: `Bearer ${options.apiKey}` }
                    : {}),
                },
                body: JSON.stringify(execution),
                signal: AbortSignal.timeout(10_000),
              },
            );
            if (!response.ok)
              throw new Error(`Refract ingestion failed: ${response.status}`);
          }
          await options.exporter?.export(execution);
          options.onComplete?.(structuredClone(execution));
        }
      } catch (recordingError) {
        try {
          options.onError?.(recordingError);
        } catch {
          /* Observability callbacks cannot mask application results. */
        }
        if (!failed && options.failOpen === false) throw recordingError;
      }
    }
  });
}
export const refract = { run, event, span };
export {
  instrumentOpenAI,
  instrumentAnthropic,
  instrumentAzureOpenAI,
  instrumentGemini,
  instrumentVertex,
  instrumentBedrock,
  instrumentCustom,
  type InstrumentOptions,
  type CustomInstrumentOptions,
  type NormalizedGeneration,
  type GenerationUsage,
} from "./instrumentation.js";
export { BatchExporter, type BatchExporterOptions } from "./exporter.js";
export { toOtlp, fromOtlp, exportOtlp } from "./otel.js";
export { toLangfuse, exportLangfuse } from "./langfuse.js";
