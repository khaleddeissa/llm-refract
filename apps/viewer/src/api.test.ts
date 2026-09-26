import { expect, it, vi, afterEach } from "vitest";
import { request } from "./api";
afterEach(() => vi.unstubAllGlobals());
it("surfaces server failures", async () => {
  vi.stubGlobal(
    "fetch",
    vi
      .fn()
      .mockResolvedValue(new Response('{"error":"blocked"}', { status: 400 })),
  );
  await expect(request("/v1/runs/x/replay", { mode: "live" })).rejects.toThrow(
    "blocked",
  );
});
it("keeps API credentials in memory and sends them on GET and POST", async () => {
  const { setApiKey } = await import("./api");
  const fetch = vi
    .fn()
    .mockImplementation(() => Promise.resolve(new Response("{}")));
  vi.stubGlobal("fetch", fetch);
  setApiKey("test-key");
  await request("/v1/search");
  expect(fetch.mock.calls[0][1].headers.Authorization).toBe("Bearer test-key");
  await request("/v1/diff", { left: "one", right: "two" });
  expect(fetch.mock.calls[1][1].headers.Authorization).toBe("Bearer test-key");
  setApiKey("");
  await request("/v1/search");
  expect(fetch.mock.calls[2][1].headers.Authorization).toBeUndefined();
});
