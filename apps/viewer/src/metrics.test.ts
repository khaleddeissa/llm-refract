import { expect, it } from "vitest";
import fixture from "../../../tests/fixtures/simple-run/execution.json" with { type: "json" };
import type { Execution } from "../../../packages/typescript/src/index.js";
import { difference, metrics } from "./metrics";
import { layout } from "./graph";
it("uses wall latency, sums available usage, and distinguishes unknown cost", () => {
  const run = structuredClone(fixture) as unknown as Execution;
  const original = metrics(run);
  expect(original.cost).toBeUndefined();
  expect(original.tokens).toBeUndefined();
  run.events[0].type = "generation";
  run.events[0].attributes = {
    input_tokens: 90,
    output_tokens: 10,
    cost_usd: 0.04,
    ttft_ms: 24,
  };
  run.events[1].attributes = {
    input_tokens: 50,
    output_tokens: 20,
    total_tokens: 100,
    cost_usd: 0.01,
    ttft_ms: 12,
    cache_read_tokens: 30,
  };
  const summary = metrics(run);
  expect(summary.tokens).toBe(200);
  expect(summary.cost).toBe(0.05);
  expect(summary.ttft).toBe(18);
  expect(summary.expensive?.id).toBe(run.events[0].id);
  expect(summary.latency).toBe(
    Date.parse(run.ended_at!) - Date.parse(run.started_at),
  );
  expect(difference(0.1, 0.05)).toBe("-50.0%");
  expect(difference(undefined, 0.05)).toBe("—");
});
it("draws only recorded parent edges, including independent roots", () => {
  const run = structuredClone(fixture) as unknown as Execution;
  run.events[1].parent_id = run.events[0].id;
  const nodes = layout(run.events);
  expect(nodes[1].x).toBeGreaterThan(nodes[0].x);
  run.events[1].parent_id = null;
  const roots = layout(run.events);
  expect(roots[1].x).toBe(roots[0].x);
});
