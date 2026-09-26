import { afterEach, expect, it, vi } from "vitest";
import fixture from "../../../tests/fixtures/simple-run/execution.json" with { type: "json" };
import {
  exportLangfuse,
  toLangfuse,
  fromOtlp,
  type Execution,
} from "../src/index.js";

afterEach(() => vi.unstubAllGlobals());

it("maps redacted recordings to Langfuse observations without changing causal IDs or caller data", () => {
  const run = structuredClone(fixture) as unknown as Execution;
  run.metadata = {
    session_id: "session-one",
    environment: "test",
    secret: "never export",
    tags: ["offline"],
  };
  run.events[1].parent_id = run.events[0].id;
  run.events[1].attributes = {
    model: "fixture",
    input_tokens: 4,
    output_tokens: 2,
    cost_usd: 0.0001,
  };
  run.events[1].input = { api_key: "private", prompt: "Hello" };
  const document = toLangfuse(run);
  const spans = (document.resourceSpans as any)[0].scopeSpans[0].spans;
  const attrs = Object.fromEntries(
    spans[1].attributes.map((attr: any) => [
      attr.key,
      attr.value.stringValue ?? attr.value.arrayValue,
    ]),
  );
  expect(attrs["langfuse.trace.name"]).toBe(run.name);
  expect(attrs["langfuse.session.id"]).toBe("session-one");
  expect(attrs["langfuse.observation.type"]).toBe("generation");
  expect(JSON.parse(attrs["langfuse.observation.usage_details"])).toEqual({
    input: 4,
    output: 2,
  });
  expect(JSON.parse(attrs["langfuse.observation.cost_details"])).toEqual({
    total: 0.0001,
  });
  expect(JSON.parse(attrs["langfuse.observation.input"])).toEqual({
    api_key: "[REDACTED]",
    prompt: "Hello",
  });
  expect(JSON.stringify(document)).not.toContain("never export");
  expect(fromOtlp(document)[0].events[1].parent_id).toBe(run.events[0].id);
  expect(run.metadata.secret).toBe("never export");
});

it("sends the configured Langfuse endpoint and Basic auth without embedding credentials in spans", async () => {
  const fetch = vi.fn().mockResolvedValue(new Response("{}"));
  vi.stubGlobal("fetch", fetch);
  await exportLangfuse(
    fixture as unknown as Execution,
    "http://langfuse.internal:3000/",
    {
      publicKey: "pk-test",
      secretKey: "sk-test",
    },
  );
  const [url, request] = fetch.mock.calls[0];
  expect(url).toBe("http://langfuse.internal:3000/api/public/otel/v1/traces");
  expect(request.headers.Authorization).toBe(
    `Basic ${Buffer.from("pk-test:sk-test").toString("base64")}`,
  );
  expect(request.headers["x-langfuse-ingestion-version"]).toBe("4");
  expect(request.body).not.toContain("sk-test");
});

it("rejects incomplete exports and surfaces authentication failures or partial ingestion", async () => {
  const run = structuredClone(fixture) as unknown as Execution;
  expect(() => toLangfuse({ ...run, status: "running" })).toThrow("completed");
  expect(() => toLangfuse({ ...run, events: [] })).toThrow(
    "at least one event",
  );
  const fetch = vi
    .fn()
    .mockResolvedValueOnce(new Response("{}", { status: 401 }))
    .mockResolvedValueOnce(
      new Response(
        JSON.stringify({
          partialSuccess: { rejectedSpans: "1", errorMessage: "quota" },
        }),
      ),
    );
  vi.stubGlobal("fetch", fetch);
  const options = { publicKey: "pk-test", secretKey: "sk-test" };
  await expect(
    exportLangfuse(run, "http://example.invalid", options),
  ).rejects.toThrow("401");
  await expect(
    exportLangfuse(run, "http://example.invalid", options),
  ).rejects.toThrow("partial success: quota");
});
