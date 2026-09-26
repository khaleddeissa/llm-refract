import { createHash } from "node:crypto";
import {
  redact,
  type Execution,
  type ExecutionEvent,
  type Json,
} from "./index.js";

type Data = Record<string, unknown>;
function object(value: unknown): Data {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new Error("Expected an OTLP object");
  return value as Data;
}
function array(value: unknown): unknown[] {
  if (value === undefined) return [];
  if (!Array.isArray(value)) throw new Error("Expected an OTLP array");
  return value;
}
function hash(value: string, length: number): string {
  return createHash("sha256").update(value).digest("hex").slice(0, length);
}
function value(input: Json): Data {
  if (input === null) return {};
  if (typeof input === "boolean") return { boolValue: input };
  if (typeof input === "number")
    return Number.isSafeInteger(input)
      ? { intValue: String(input) }
      : { doubleValue: input };
  if (typeof input === "string") return { stringValue: input };
  if (Array.isArray(input)) return { arrayValue: { values: input.map(value) } };
  return { kvlistValue: { values: attributes(input) } };
}
function attributes(input: Record<string, Json>): Data[] {
  return Object.entries(input).map(([key, item]) => ({
    key,
    value: value(item),
  }));
}
function decode(input: unknown): Json {
  const item = object(input);
  if ("stringValue" in item) return String(item.stringValue);
  if ("boolValue" in item) return Boolean(item.boolValue);
  if ("intValue" in item) {
    const result = Number(item.intValue);
    return Number.isSafeInteger(result) ? result : String(item.intValue);
  }
  if ("doubleValue" in item) return Number(item.doubleValue);
  if ("arrayValue" in item)
    return array(object(item.arrayValue).values).map(decode);
  if ("kvlistValue" in item)
    return decodeAttributes(object(item.kvlistValue).values);
  return null;
}
function decodeAttributes(input: unknown): Record<string, Json> {
  return Object.fromEntries(
    array(input).map((entry) => {
      const item = object(entry);
      return [String(item.key), decode(item.value)];
    }),
  );
}
function nanos(timestamp: string): bigint {
  const ms = Date.parse(timestamp);
  if (!Number.isFinite(ms)) throw new Error("Invalid execution timestamp");
  return BigInt(ms) * 1_000_000n;
}
function timestamp(nano: unknown): string {
  return new Date(Number(BigInt(String(nano)) / 1_000_000n)).toISOString();
}
const semanticKeys: Record<string, string> = {
  model: "gen_ai.request.model",
  provider: "gen_ai.provider.name",
  input_tokens: "gen_ai.usage.input_tokens",
  output_tokens: "gen_ai.usage.output_tokens",
};
/** Encode a completed execution as OTLP/HTTP JSON traces without an OTel dependency. */
export function toOtlp(
  recording: Execution,
  serviceName = "llm-refract",
): Data {
  const run = redact(
    structuredClone(recording) as unknown as Json,
  ) as unknown as Execution;
  const spans = run.events.map((event) => {
    const start = nanos(event.timestamp);
    const attrs: Record<string, Json> = {
      "refract.event.id": event.id,
      "refract.event.type": event.type,
      "refract.event.input": JSON.stringify(event.input ?? null),
      "refract.event.output": JSON.stringify(event.output ?? null),
      "refract.event.attributes": JSON.stringify(event.attributes ?? {}),
      "refract.event.replay_policy": event.replay_policy ?? "RECORDED",
    };
    for (const [source, target] of Object.entries(semanticKeys))
      if (event.attributes?.[source] !== undefined)
        attrs[target] = event.attributes[source];
    return {
      traceId: hash(run.id, 32),
      spanId: hash(event.id, 16),
      ...(event.parent_id ? { parentSpanId: hash(event.parent_id, 16) } : {}),
      name: event.name,
      kind: event.type === "generation" ? 3 : 1,
      startTimeUnixNano: String(start),
      endTimeUnixNano: String(
        start + BigInt(Math.round((event.duration_ms ?? 0) * 1_000_000)),
      ),
      attributes: attributes(attrs),
      status: { code: event.status === "failed" ? 2 : 1 },
    };
  });
  return {
    resourceSpans: [
      {
        resource: {
          attributes: attributes({
            "service.name": serviceName,
            "refract.run.id": run.id,
            "refract.run.name": run.name,
            "refract.run.metadata": JSON.stringify(run.metadata),
            "refract.run.started_at": run.started_at,
            "refract.run.ended_at": run.ended_at,
            "refract.run.status": run.status,
          }),
        },
        scopeSpans: [{ scope: { name: "llm-refract" }, spans }],
      },
    ],
  };
}
/** Import one execution per trace, keeping external parent IDs, links and span events. */
export function fromOtlp(document: unknown): Execution[] {
  const groups = new Map<
    string,
    { span: Data; resource: Record<string, Json> }[]
  >();
  for (const resource of array(object(document).resourceSpans)) {
    const item = object(resource);
    const attrs = decodeAttributes(object(item.resource ?? {}).attributes);
    for (const scope of array(item.scopeSpans))
      for (const rawSpan of array(object(scope).spans)) {
        const span = { ...object(rawSpan) };
        const traceId = String(span.traceId).toLowerCase();
        const spanId = String(span.spanId).toLowerCase();
        if (!/^[a-f0-9]{32}$/.test(traceId) || /^0+$/.test(traceId))
          throw new Error("Invalid OTLP trace ID");
        if (!/^[a-f0-9]{16}$/.test(spanId) || /^0+$/.test(spanId))
          throw new Error("Invalid OTLP span ID");
        span.spanId = spanId;
        if (span.parentSpanId)
          span.parentSpanId = String(span.parentSpanId).toLowerCase();
        const entries = groups.get(traceId) ?? [];
        entries.push({ span, resource: attrs });
        groups.set(traceId, entries);
      }
  }
  return [...groups].map(([traceId, entries]) => {
    const resource = entries[0].resource;
    const starts = entries.map(({ span }) =>
      BigInt(String(span.startTimeUnixNano)),
    );
    const ends = entries.map(({ span }) =>
      BigInt(String(span.endTimeUnixNano)),
    );
    const run: Execution = {
      spec_version: "refract.execution.v1",
      id: String(resource["refract.run.id"] ?? `run_otel_${traceId}`),
      name: String(
        resource["refract.run.name"] ??
          resource["service.name"] ??
          "OTel trace",
      ),
      status: (resource["refract.run.status"] ??
        "completed") as Execution["status"],
      started_at: String(
        resource["refract.run.started_at"] ??
          timestamp(starts.reduce((a, b) => (a < b ? a : b))),
      ),
      ended_at:
        resource["refract.run.ended_at"] === null
          ? null
          : String(
              resource["refract.run.ended_at"] ??
                timestamp(ends.reduce((a, b) => (a > b ? a : b))),
            ),
      metadata: object(
        JSON.parse(String(resource["refract.run.metadata"] ?? "{}")),
      ) as Record<string, Json>,
      events: [],
    };
    const mapped = new Map<string, string>();
    for (const { span } of entries) {
      const id = String(span.spanId);
      if (mapped.has(id)) throw new Error("Duplicate OTLP span ID");
      mapped.set(
        id,
        String(
          decodeAttributes(span.attributes)["refract.event.id"] ?? `evt_${id}`,
        ),
      );
    }
    if (new Set(mapped.values()).size !== mapped.size)
      throw new Error("Duplicate mapped event IDs");
    const pending = [...entries];
    const emitted = new Set<string>();
    while (pending.length) {
      let changed = false;
      for (const entry of [...pending]) {
        const { span, resource: spanResource } = entry;
        const parent = span.parentSpanId
          ? String(span.parentSpanId)
          : undefined;
        if (parent && mapped.has(parent) && !emitted.has(parent)) continue;
        const attrs = decodeAttributes(span.attributes);
        const values = object(
          JSON.parse(String(attrs["refract.event.attributes"] ?? "{}")),
        ) as Record<string, Json>;
        for (const [target, source] of Object.entries(semanticKeys))
          if (attrs[source] !== undefined) values[target] = attrs[source];
        if (
          values.provider === undefined &&
          attrs["gen_ai.system"] !== undefined
        )
          values.provider = attrs["gen_ai.system"];
        if (attrs["refract.event.id"] === undefined) {
          values["otel.attributes"] = attrs;
          values["otel.resource"] = spanResource;
        }
        if (parent && !mapped.has(parent))
          values["otel.external_parent_id"] = parent;
        for (const field of ["events", "links"])
          if (span[field] !== undefined)
            values[`otel.${field}`] = span[field] as Json;
        const start = BigInt(String(span.startTimeUnixNano));
        const end = BigInt(String(span.endTimeUnixNano));
        if (end < start) throw new Error("OTLP span ends before it starts");
        const status = object(span.status ?? {}).code;
        const failed = status === 2 || status === "STATUS_CODE_ERROR";
        if (failed) run.status = "failed";
        run.events.push({
          id: mapped.get(String(span.spanId))!,
          run_id: run.id,
          parent_id: parent ? (mapped.get(parent) ?? null) : null,
          type: (attrs["refract.event.type"] ??
            (values.model
              ? "generation"
              : "decision")) as ExecutionEvent["type"],
          name: String(span.name),
          timestamp: timestamp(start),
          duration_ms: Number(end - start) / 1_000_000,
          status: failed ? "failed" : "completed",
          input: JSON.parse(
            String(attrs["refract.event.input"] ?? "null"),
          ) as Json,
          output: JSON.parse(
            String(attrs["refract.event.output"] ?? "null"),
          ) as Json,
          attributes: values,
          replay_policy: (attrs["refract.event.replay_policy"] ??
            "RECORDED") as ExecutionEvent["replay_policy"],
        });
        emitted.add(String(span.spanId));
        pending.splice(pending.indexOf(entry), 1);
        changed = true;
      }
      if (!changed) throw new Error("OTLP parent cycle");
    }
    return redact(run as unknown as Json) as unknown as Execution;
  });
}
/** Explicit synchronous-in-flow export; pass the collector base URL or /v1/traces. */
export async function exportOtlp(
  run: Execution,
  endpoint: string,
  options: { headers?: Record<string, string>; timeoutMs?: number } = {},
): Promise<void> {
  const base = endpoint.replace(/\/$/, "");
  const response = await fetch(
    base.endsWith("/v1/traces") ? base : `${base}/v1/traces`,
    {
      method: "POST",
      headers: { "Content-Type": "application/json", ...options.headers },
      body: JSON.stringify(toOtlp(run)),
      signal: AbortSignal.timeout(options.timeoutMs ?? 10_000),
    },
  );
  if (!response.ok) throw new Error(`OTLP export failed: ${response.status}`);
  const result = object(await response.json());
  const partial = object(result.partialSuccess ?? {});
  if (BigInt(String(partial.rejectedSpans ?? "0")) > 0n || partial.errorMessage)
    throw new Error(
      `OTLP partial success: ${String(partial.errorMessage ?? partial.rejectedSpans)}`,
    );
}
