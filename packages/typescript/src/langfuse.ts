import { redact, type Execution, type Json } from "./index.js";
import { toOtlp } from "./otel.js";

/** Convert a completed recording to Langfuse's OTLP span conventions. */
export function toLangfuse(recording: Execution) {
  if (
    recording.status === "running" ||
    !recording.ended_at ||
    recording.events.some((event) => event.status === "running")
  )
    throw new Error("Langfuse export requires a completed recording");
  if (!recording.events.length)
    throw new Error("Langfuse export requires at least one event");
  const run = redact(
    structuredClone(recording) as unknown as Json,
  ) as unknown as Execution;
  const document = toOtlp(run);
  const resources = document.resourceSpans as {
    scopeSpans: { spans: { attributes: unknown[] }[] }[];
  }[];
  const types: Record<string, string> = {
    generation: "generation",
    "tool.call": "tool",
    retrieval: "retriever",
  };
  for (const [index, event] of run.events.entries()) {
    const attrs: Record<string, Json> = {
      "langfuse.trace.name": run.name,
      "langfuse.observation.type": types[event.type] ?? "span",
      "langfuse.observation.input": JSON.stringify(event.input ?? null),
      "langfuse.observation.output": JSON.stringify(event.output ?? null),
    };
    for (const [source, target] of Object.entries({
      user_id: "langfuse.user.id",
      session_id: "langfuse.session.id",
      environment: "langfuse.environment",
      tags: "langfuse.trace.tags",
      version: "langfuse.version",
      release: "langfuse.release",
    }))
      if (run.metadata[source] !== undefined)
        attrs[target] = run.metadata[source];
    for (const [key, item] of Object.entries(run.metadata))
      attrs[`langfuse.trace.metadata.${key}`] = JSON.stringify(item);
    const metrics = event.attributes ?? {};
    if (event.type === "generation") {
      if (typeof metrics.model === "string")
        attrs["langfuse.observation.model.name"] = metrics.model;
      const usage: Record<string, Json> = {};
      for (const [source, target] of Object.entries({
        input_tokens: "input",
        output_tokens: "output",
        total_tokens: "total",
      }))
        if (typeof metrics[source] === "number")
          usage[target] = metrics[source];
      if (Object.keys(usage).length)
        attrs["langfuse.observation.usage_details"] = JSON.stringify(usage);
      if (typeof metrics.cost_usd === "number")
        attrs["langfuse.observation.cost_details"] = JSON.stringify({
          total: metrics.cost_usd,
        });
    }
    resources[0].scopeSpans[0].spans[index].attributes.push(
      ...Object.entries(attrs).map(([key, item]) => ({
        key,
        value: Array.isArray(item)
          ? {
              arrayValue: {
                values: item.map((value) => ({ stringValue: String(value) })),
              },
            }
          : { stringValue: String(item) },
      })),
    );
  }
  return document;
}

/** Explicit export to a cloud or self-hosted Langfuse URL; never called automatically. */
export async function exportLangfuse(
  recording: Execution,
  baseUrl: string,
  options: { publicKey: string; secretKey: string; timeoutMs?: number },
): Promise<void> {
  if (!options.publicKey || !options.secretKey)
    throw new Error("Langfuse publicKey and secretKey are required");
  const response = await fetch(
    `${baseUrl.replace(/\/$/, "")}/api/public/otel/v1/traces`,
    {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Basic ${Buffer.from(`${options.publicKey}:${options.secretKey}`).toString("base64")}`,
        "x-langfuse-ingestion-version": "4",
      },
      body: JSON.stringify(toLangfuse(recording)),
      signal: AbortSignal.timeout(options.timeoutMs ?? 10_000),
    },
  );
  if (!response.ok)
    throw new Error(`Langfuse export failed: ${response.status}`);
  const result = (await response.json()) as {
    partialSuccess?: { rejectedSpans?: string; errorMessage?: string };
  };
  if (
    BigInt(result.partialSuccess?.rejectedSpans ?? "0") > 0n ||
    result.partialSuccess?.errorMessage
  )
    throw new Error(
      `Langfuse partial success: ${result.partialSuccess?.errorMessage ?? result.partialSuccess?.rejectedSpans}`,
    );
}
