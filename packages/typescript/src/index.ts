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
