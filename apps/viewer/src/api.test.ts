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
