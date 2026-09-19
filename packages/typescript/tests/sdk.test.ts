import { describe, it, expect } from "vitest";
import { refract, pack, unpack, type Execution } from "../src/index.js";

describe("recording", () => {
  it("isolates concurrent runs and redacts before capture", async () => {
    const runs: Execution[] = [];
    await Promise.all(
      ["a", "b"].map((name) =>
        refract.run(
          name,
          async () => {
            await Promise.resolve();
            refract.event({
              type: "tool.call",
              name,
              input: { api_key: "secret" },
            });
          },
          {
            onComplete: (r) => {
              runs.push(r);
            },
          },
        ),
      ),
    );
    expect(runs.map((r) => r.events[0].name).sort()).toEqual(["a", "b"]);
    expect(runs[0].events[0].input).toEqual({ api_key: "[REDACTED]" });
    expect(pack(runs[0])).toEqual(pack(runs[0]));
    expect(unpack(pack(runs[0]))).toEqual(runs[0]);
    const bad = Buffer.from(pack(runs[0]));
    bad[bad.length - 2] = 33;
    expect(() => unpack(bad)).toThrow("checksum mismatch");
    expect(() => refract.event({ type: "error", name: "outside" })).toThrow();
  });
  it("preserves application failures", async () => {
    let run: Execution | undefined;
    await expect(
      refract.run(
        "failure",
        () => {
          throw new Error("original");
        },
        {
          onComplete: (r) => {
            run = r;
          },
        },
      ),
    ).rejects.toThrow("original");
    expect(run?.status).toBe("failed");
    expect(run?.events[0].status).toBe("failed");
  });
});
