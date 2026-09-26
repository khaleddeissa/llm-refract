import { afterEach, expect, it, vi } from "vitest";
import fixture from "../../../tests/fixtures/simple-run/execution.json" with { type: "json" };
import { exportOtlp, fromOtlp, toOtlp, type Execution } from "../src/index.js";

afterEach(() => vi.unstubAllGlobals());
it("round trips canonical graph, payloads and redacted token metrics through OTLP JSON", () => {
  const run = structuredClone(fixture) as unknown as Execution;
  run.events[1].parent_id = run.events[0].id;
  run.events[1].attributes = {
    model: "test",
    input_tokens: 7,
    access_token: "private",
  };
  const document = toOtlp(run);
  const spans = (document.resourceSpans as any)[0].scopeSpans[0].spans;
  expect(spans[0].traceId).toMatch(/^[0-9a-f]{32}$/);
  expect(spans[1].parentSpanId).toBe(spans[0].spanId);
  expect(typeof spans[0].startTimeUnixNano).toBe("string");
  const restored = fromOtlp(document)[0];
  expect(restored.id).toBe(run.id);
  expect(restored.events[1].parent_id).toBe(run.events[0].id);
  expect(restored.events[1].output).toEqual(run.events[1].output);
  expect(restored.events[1].attributes).toEqual({
    model: "test",
    input_tokens: 7,
    access_token: "[REDACTED]",
  });
});
it("imports unordered external OTel spans and rejects duplicate IDs and cycles", () => {
  const run = structuredClone(fixture) as unknown as Execution;
  run.events[1].parent_id = run.events[0].id;
  const document = toOtlp(run);
  const spans = (document.resourceSpans as any)[0].scopeSpans[0].spans;
  spans.reverse();
  expect(fromOtlp(document)[0].events.map((event) => event.id)).toEqual(
    run.events.map((event) => event.id),
  );
  spans[1].parentSpanId = spans[0].spanId;
  expect(() => fromOtlp(document)).toThrow("parent cycle");
  delete spans[1].parentSpanId;
  spans.push(spans[0]);
  expect(() => fromOtlp(document)).toThrow("Duplicate OTLP span ID");
});
it("posts redacted OTLP JSON and reports collector partial rejection", async () => {
  const fetch = vi
    .fn()
    .mockResolvedValueOnce(new Response("{}"))
    .mockResolvedValueOnce(
      new Response(
        JSON.stringify({
          partialSuccess: { rejectedSpans: "1", errorMessage: "quota" },
        }),
      ),
    );
  vi.stubGlobal("fetch", fetch);
  await exportOtlp(fixture as unknown as Execution, "http://collector:4318", {
    headers: { Authorization: "Bearer test" },
  });
  expect(fetch.mock.calls[0][0]).toBe("http://collector:4318/v1/traces");
  expect(fetch.mock.calls[0][1].headers.Authorization).toBe("Bearer test");
  await expect(
    exportOtlp(
      fixture as unknown as Execution,
      "http://collector:4318/v1/traces",
    ),
  ).rejects.toThrow("partial success: quota");
});
