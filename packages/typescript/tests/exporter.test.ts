import { mkdtemp, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, expect, it, vi } from "vitest";
import { BatchExporter, refract } from "../src/index.js";
afterEach(() => vi.unstubAllGlobals());
it("exports bounded authenticated batches and retries temporary failures", async () => {
  const fetch = vi
    .fn()
    .mockResolvedValueOnce(new Response("busy", { status: 503 }))
    .mockImplementation(() => Promise.resolve(new Response("{}")));
  vi.stubGlobal("fetch", fetch);
  const exporter = new BatchExporter({
    endpoint: "http://refract.test",
    apiKey: "test",
    batchSize: 2,
    maxQueueSize: 2,
    retryDelayMs: 0,
  });
  await refract.run("one", () => 1, { exporter });
  await refract.run("two", () => 2, { exporter });
  await refract.run("three", () => 3, { exporter });
  expect(exporter.stats.dropped).toBe(1);
  await exporter.shutdown();
  expect(fetch).toHaveBeenCalledTimes(2);
  expect(fetch.mock.calls[0][0]).toBe("http://refract.test/v1/runs/batch");
  expect(fetch.mock.calls[0][1].headers.Authorization).toBe("Bearer test");
  expect(JSON.parse(fetch.mock.calls[0][1].body).runs).toHaveLength(2);
  expect(exporter.stats.exported).toBe(2);
});
it("recovers durable redacted snapshots in a new exporter after an outage", async () => {
  const directory = await mkdtemp(join(tmpdir(), "refract-spool-"));
  try {
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("offline")));
    const first = new BatchExporter({
      endpoint: "http://refract.test",
      spoolDirectory: directory,
      maxAttempts: 1,
    });
    await expect(
      refract.run("survives", () => "ok", {
        exporter: first,
        metadata: { password: "secret" },
      }),
    ).resolves.toBe("ok");
    await first.shutdown();
    expect(await readdir(directory)).toHaveLength(1);
    const fetch = vi.fn().mockResolvedValue(new Response("{}"));
    vi.stubGlobal("fetch", fetch);
    const second = new BatchExporter({
      endpoint: "http://refract.test",
      spoolDirectory: directory,
    });
    await second.shutdown();
    const body = JSON.parse(fetch.mock.calls[0][1].body);
    expect(body.runs[0].metadata.password).toBe("[REDACTED]");
    expect(await readdir(directory)).toHaveLength(0);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
it("fails open by default and exposes strict opt-in behavior", async () => {
  vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("offline")));
  const onError = vi.fn();
  await expect(
    refract.run("ok", () => 42, { endpoint: "http://offline.test", onError }),
  ).resolves.toBe(42);
  expect(onError).toHaveBeenCalledTimes(1);
  await expect(
    refract.run("strict", () => 42, {
      endpoint: "http://offline.test",
      failOpen: false,
    }),
  ).rejects.toThrow("offline");
});

it("durable export persists before resolution and rejects capacity failure", async () => {
  const directory = await mkdtemp(join(tmpdir(), "refract-acceptance-"));
  vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("offline")));
  const exporter = new BatchExporter({
    endpoint: "http://fixture.invalid",
    spoolDirectory: directory,
    durable: true,
    maxAttempts: 1,
    flushIntervalMs: 60_000,
  });
  try {
    await refract.run("durable", () => 1, { exporter, failOpen: false });
    expect(await readdir(directory)).toHaveLength(1);
    expect(exporter.stats.accepted).toBe(1);
    const full = new BatchExporter({
      endpoint: "http://fixture.invalid",
      spoolDirectory: directory,
      durable: true,
      maxSpoolBytes: 1,
      maxQueueBytes: 1,
      maxAttempts: 1,
    });
    await expect(
      refract.run("overflow", () => 1, { exporter: full, failOpen: false }),
    ).rejects.toThrow("capacity");
    await full.shutdown();
  } finally {
    await exporter.shutdown();
    await rm(directory, { recursive: true, force: true });
  }
});

it("serializes concurrent acceptance and rejects conflicting snapshots", async () => {
  const directory = await mkdtemp(join(tmpdir(), "refract-concurrent-spool-"));
  vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("offline")));
  const exporter = new BatchExporter({
    endpoint: "http://fixture.invalid",
    spoolDirectory: directory,
    durable: true,
    maxAttempts: 1,
    flushIntervalMs: 60_000,
  });
  try {
    let snapshot!: import("../src/index.js").Execution;
    await refract.run("fixture", () => 1, {
      onComplete: (value) => {
        snapshot = value;
      },
    });
    await Promise.all([exporter.export(snapshot), exporter.export(snapshot)]);
    expect(exporter.stats.accepted).toBe(1);
    expect(await readdir(directory)).toHaveLength(1);
    await expect(
      exporter.export({ ...snapshot, name: "different" }),
    ).rejects.toThrow("different content");
    expect(exporter.stats.accepted).toBe(1);
  } finally {
    await exporter.shutdown();
    await rm(directory, { recursive: true, force: true });
  }
});
