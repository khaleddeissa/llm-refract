import { readFileSync } from "node:fs";
import { afterEach, expect, it, vi } from "vitest";
import { pack, unpack, refract } from "../src/index.js";
const fixture = readFileSync(
  new URL("../../../examples/artifacts/demo.rfr", import.meta.url),
);
afterEach(() => vi.unstubAllGlobals());
it("reads the shared Python text fixture and preserves Unicode", () => {
  const run = unpack(fixture);
  expect(run.id).toBe("demo-1");
  run.events[0].output = { text: "مرحبا — hello" };
  const text = new TextDecoder().decode(pack(run));
  expect(text).toContain("مرحبا");
  expect(JSON.parse(text.slice(text.indexOf("\n") + 1)).events).toEqual(
    run.events,
  );
});
it("rejects corrupt and unsupported recordings", () => {
  expect(() =>
    unpack(Buffer.from(fixture.toString().replace("demo-1", "demo-2"))),
  ).toThrow("checksum");
  expect(() =>
    unpack(
      Buffer.from(
        fixture
          .toString()
          .replace("refract.artifact.v1", "refract.artifact.v9"),
      ),
    ),
  ).toThrow("unsupported");
});
it("reports failed remote persistence without hiding application exceptions", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn().mockResolvedValue(new Response("unavailable", { status: 503 })),
  );
  await expect(
    refract.run("transport", () => 42, { endpoint: "http://refract.test" }),
  ).rejects.toThrow("503");
  await expect(
    refract.run(
      "app",
      () => {
        throw new Error("application failure");
      },
      { endpoint: "http://refract.test" },
    ),
  ).rejects.toThrow("application failure");
});
