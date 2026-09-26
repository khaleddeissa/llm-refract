import type {
  Execution,
  ExecutionEvent,
} from "../../../packages/typescript/src/index.js";
export function metric(event: ExecutionEvent, key: string): number | undefined {
  const value = event.attributes?.[key];
  return typeof value === "number" && Number.isFinite(value) && value >= 0
    ? value
    : undefined;
}
export function metrics(run: Execution) {
  const generations = run.events.filter((event) => event.type === "generation");
  const sum = (key: string) => {
    const values = generations
      .map((event) => metric(event, key))
      .filter((value): value is number => value !== undefined);
    return values.length
      ? values.reduce((total, value) => total + value, 0)
      : undefined;
  };
  const firstTokens = generations
    .map((event) => metric(event, "ttft_ms"))
    .filter((value): value is number => value !== undefined);
  const latency = run.ended_at
    ? Date.parse(run.ended_at) - Date.parse(run.started_at)
    : undefined;
  const totalTokens = generations
    .map(
      (event) =>
        metric(event, "total_tokens") ??
        (metric(event, "input_tokens") !== undefined ||
        metric(event, "output_tokens") !== undefined
          ? (metric(event, "input_tokens") ?? 0) +
            (metric(event, "output_tokens") ?? 0)
          : undefined),
    )
    .filter((value): value is number => value !== undefined);
  return {
    cost: sum("cost_usd"),
    latency:
      latency !== undefined && Number.isFinite(latency)
        ? Math.max(0, latency)
        : undefined,
    tokens: totalTokens.length
      ? totalTokens.reduce((a, b) => a + b, 0)
      : undefined,
    input: sum("input_tokens"),
    output: sum("output_tokens"),
    cache: sum("cache_read_tokens"),
    ttft: firstTokens.length
      ? firstTokens.reduce((a, b) => a + b, 0) / firstTokens.length
      : undefined,
    costCoverage: run.events.filter(
      (event) =>
        event.type === "generation" && metric(event, "cost_usd") !== undefined,
    ).length,
    usageCoverage: run.events.filter(
      (event) =>
        event.type === "generation" &&
        metric(event, "input_tokens") !== undefined &&
        metric(event, "output_tokens") !== undefined,
    ).length,
    generations: run.events.filter((event) => event.type === "generation")
      .length,
    tools: run.events.filter((event) => event.type === "tool.call").length,
    expensive: generations
      .filter((event) => metric(event, "cost_usd") !== undefined)
      .sort((a, b) => metric(b, "cost_usd")! - metric(a, "cost_usd")!)[0],
    slowest: [...run.events].sort(
      (a, b) => (b.duration_ms ?? 0) - (a.duration_ms ?? 0),
    )[0],
  };
}
export function number(value: number | undefined, digits = 0): string {
  return value === undefined
    ? "—"
    : value.toLocaleString(undefined, { maximumFractionDigits: digits });
}
export function difference(
  before: number | undefined,
  after: number | undefined,
): string {
  if (before === undefined || after === undefined) return "—";
  if (before === after) return "0%";
  return before === 0
    ? "new"
    : `${after > before ? "+" : ""}${(((after - before) / before) * 100).toFixed(1)}%`;
}
