import { AsyncLocalStorage } from "node:async_hooks";
import { createHash, randomUUID } from "node:crypto";
import { writeFile } from "node:fs/promises";
import { zipSync } from "fflate";
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
export function redact(value: Json): Json {
  if (Array.isArray(value)) return value.map(redact);
  if (value !== null && typeof value === "object")
    return Object.fromEntries(
      Object.entries(value).map(([key, v]) => [
        key,
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
}>();
export function event(input: EventInput): string {
  const state = context.getStore();
  if (!state?.active) throw new Error("event requires an active refract.run");
  if (!input.name.trim() || (input.duration_ms ?? 0) < 0)
    throw new Error("invalid event name/duration");
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
export function pack(execution: Execution): Uint8Array {
  const encode = (v: unknown) => new TextEncoder().encode(JSON.stringify(v));
  const safe = redact(execution as unknown as Json) as unknown as Execution;
  const files: Record<string, Uint8Array> = {
    "execution.json": encode({ ...safe, events: [] }),
    "events.jsonl": new TextEncoder().encode(
      safe.events.map((e) => JSON.stringify(e) + "\n").join(""),
    ),
  };
  if (Object.values(files).reduce((n, b) => n + b.length, 0) > 16 * 1024 * 1024)
    throw new Error("artifact exceeds size limit");
  const manifest = {
    spec_version: execution.spec_version,
    files: Object.fromEntries(
      Object.entries(files).map(([n, b]) => [
        n,
        createHash("sha256").update(b).digest("hex"),
      ]),
    ),
  };
  return zipSync(
    { "manifest.json": encode(manifest), ...files },
    { level: 0, mtime: new Date(1980, 0, 1) },
  );
}
export async function run<T>(
  name: string,
  fn: () => T | Promise<T>,
  options: {
    path?: string;
    endpoint?: string;
    metadata?: Record<string, Json>;
    onComplete?: (execution: Execution) => void;
  } = {},
): Promise<T> {
  if (!name.trim()) throw new Error("run name cannot be empty");
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
  const state = { execution, active: true };
  return context.run(state, async () => {
    let failed = false;
    try {
      const result = await fn();
      execution.status = "completed";
      return result;
    } catch (error) {
      failed = true;
      execution.status = "failed";
      event({ type: "error", name: "Execution failed", status: "failed" });
      throw error;
    } finally {
      state.active = false;
      execution.ended_at = new Date().toISOString();
      try {
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
              headers: { "Content-Type": "application/json" },
              body: JSON.stringify(execution),
              signal: AbortSignal.timeout(10_000),
            },
          );
          if (!response.ok)
            throw new Error(`Refract ingestion failed: ${response.status}`);
        }
        options.onComplete?.(structuredClone(execution));
      } catch (recordingError) {
        if (!failed) throw recordingError;
      }
    }
  });
}
export const refract = { run, event };
