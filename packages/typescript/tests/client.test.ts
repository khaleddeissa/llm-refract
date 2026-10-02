import { afterEach, expect, test, vi } from "vitest";
import { RefractClient } from "../src/index.js";

afterEach(() => vi.unstubAllGlobals());
test("semantic service client scopes requests to configured endpoint and disables redirects", async () => {
  const fetch = vi
    .fn()
    .mockResolvedValue(new Response(JSON.stringify({ runs: [], matches: [] })));
  vi.stubGlobal("fetch", fetch);
  const client = new RefractClient("http://localhost:8000", "test-key");
  expect(
    (await client.searchText("refund", { profile: "local" })).runs,
  ).toEqual([]);
  expect(fetch.mock.calls[0][0]).toBe("http://localhost:8000/v1/search/text");
  const options = fetch.mock.calls[0][1];
  expect(options.headers.Authorization).toBe("Bearer test-key");
  expect(options.redirect).toBe("error");
  expect(JSON.parse(options.body)).toEqual({
    query: "refund",
    profile: "local",
  });
  await expect(client.request("https://untrusted.invalid")).rejects.toThrow(
    "/v1/",
  );
  expect(() => new RefractClient("https://secret@example.invalid")).toThrow();
});

test("provider replay and model grading preserve opt-in and event approvals", async () => {
  const fetch = vi
    .fn()
    .mockImplementation(
      async () => new Response(JSON.stringify({ id: "branch" })),
    );
  vi.stubGlobal("fetch", fetch);
  const client = new RefractClient();
  await client.rerun("a/b", {
    profile: "local",
    from_event: "first",
    allow_live: true,
    approved_events: ["tool"],
    reuse_recorded: ["tool"],
  });
  expect(fetch.mock.calls[0][0]).toContain("/v1/runs/a%2Fb/rerun");
  expect(JSON.parse(fetch.mock.calls[0][1].body)).toMatchObject({
    allow_live: true,
    approved_events: ["tool"],
    reuse_recorded: ["tool"],
  });
  await client.compare("a", "b", { grader: "domain" });
  expect(JSON.parse(fetch.mock.calls[1][1].body).allow_live).toBeUndefined();
});
