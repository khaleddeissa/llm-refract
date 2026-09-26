import {
  mkdir,
  readFile,
  readdir,
  rename,
  unlink,
  writeFile,
} from "node:fs/promises";
import { join } from "node:path";
import { createHash, randomUUID } from "node:crypto";
import { redact, type Execution, type Json } from "./index.js";

export interface BatchExporterOptions {
  endpoint: string;
  apiKey?: string;
  batchSize?: number;
  maxQueueSize?: number;
  flushIntervalMs?: number;
  maxAttempts?: number;
  retryDelayMs?: number;
  timeoutMs?: number;
  /** Dedicated directory for redacted retry snapshots. Use one exporter per directory. */
  spoolDirectory?: string;
  onError?: (error: unknown) => void;
}
/** Bounded background ingestion. Call shutdown() before terminating the process. */
export class BatchExporter {
  private queue: Execution[] = [];
  private flushing?: Promise<void>;
  private pending = new Set<Promise<void>>();
  private initialized?: Promise<void>;
  private timer: ReturnType<typeof setInterval>;
  private closed = false;
  private reserved = 0;
  readonly stats = { accepted: 0, exported: 0, dropped: 0, failures: 0 };
  private readonly config: Required<
    Omit<BatchExporterOptions, "apiKey" | "spoolDirectory" | "onError">
  > &
    BatchExporterOptions;
  constructor(options: BatchExporterOptions) {
    this.config = {
      batchSize: 32,
      maxQueueSize: 1024,
      flushIntervalMs: 1000,
      maxAttempts: 3,
      retryDelayMs: 100,
      timeoutMs: 10_000,
      ...options,
    };
    for (const key of [
      "batchSize",
      "maxQueueSize",
      "flushIntervalMs",
      "maxAttempts",
      "timeoutMs",
    ] as const)
      if (!Number.isInteger(this.config[key]) || this.config[key] < 1)
        throw new Error(`${key} must be a positive integer`);
    if (
      this.config.retryDelayMs < 0 ||
      !Number.isFinite(this.config.retryDelayMs)
    )
      throw new Error("retryDelayMs must be nonnegative");
    const endpoint = new URL(options.endpoint);
    if (!["http:", "https:"].includes(endpoint.protocol))
      throw new Error("endpoint must use HTTP(S)");
    this.timer = setInterval(() => {
      void this.flush();
    }, this.config.flushIntervalMs);
    this.timer.unref();
  }
  private report(error: unknown) {
    this.stats.failures++;
    try {
      this.config.onError?.(error);
    } catch {
      /* diagnostics cannot break application calls */
    }
  }
  private filename(run: Execution): string {
    return join(
      this.config.spoolDirectory!,
      `${createHash("sha256").update(run.id).digest("hex")}.json`,
    );
  }
  private initialize(): Promise<void> {
    return (this.initialized ??= (async () => {
      if (!this.config.spoolDirectory) return;
      await mkdir(this.config.spoolDirectory, { recursive: true, mode: 0o700 });
      for (const filename of await readdir(this.config.spoolDirectory)) {
        if (
          !/^[a-f0-9]{64}\.json$/.test(filename) ||
          this.queue.length >= this.config.maxQueueSize
        )
          continue;
        try {
          const run = JSON.parse(
            await readFile(join(this.config.spoolDirectory, filename), "utf8"),
          ) as Execution;
          if (
            run.spec_version !== "refract.execution.v1" ||
            !Array.isArray(run.events)
          )
            throw new Error("Invalid spool execution");
          this.queue.push(run);
        } catch (error) {
          this.report(error);
        }
      }
    })());
  }
  export(execution: Execution): Promise<void> {
    if (this.closed) {
      this.stats.dropped++;
      return Promise.resolve();
    }
    this.reserved++;
    const work = (async () => {
      try {
        await this.initialize();
        if (this.queue.length + this.reserved > this.config.maxQueueSize) {
          this.stats.dropped++;
          return;
        }
        const run = redact(
          structuredClone(execution) as unknown as Json,
        ) as unknown as Execution;
        if (this.queue.some((existing) => existing.id === run.id)) return;
        if (this.config.spoolDirectory) {
          const filename = this.filename(run);
          const temporary = `${filename}.${randomUUID()}.tmp`;
          await writeFile(temporary, JSON.stringify(run), {
            mode: 0o600,
            flag: "wx",
          });
          await rename(temporary, filename);
        }
        this.queue.push(run);
        this.stats.accepted++;
      } catch (error) {
        this.stats.dropped++;
        this.report(error);
      } finally {
        this.reserved--;
      }
    })();
    this.pending.add(work);
    void work.finally(() => {
      this.pending.delete(work);
    });
    return work;
  }
  flush(): Promise<void> {
    return (this.flushing ??= this.drain().finally(() => {
      this.flushing = undefined;
    }));
  }
  private async drain() {
    try {
      await Promise.all([...this.pending]);
      await this.initialize();
      while (this.queue.length) {
        const batch = this.queue.slice(0, this.config.batchSize);
        let sent = false;
        for (let attempt = 0; attempt < this.config.maxAttempts; attempt++) {
          try {
            const response = await fetch(
              `${this.config.endpoint.replace(/\/$/, "")}/v1/runs/batch`,
              {
                method: "POST",
                headers: {
                  "Content-Type": "application/json",
                  ...(this.config.apiKey
                    ? { Authorization: `Bearer ${this.config.apiKey}` }
                    : {}),
                },
                body: JSON.stringify({ runs: batch }),
                signal: AbortSignal.timeout(this.config.timeoutMs),
              },
            );
            if (response.ok) {
              sent = true;
              break;
            }
            if (response.status < 500 && response.status !== 429) {
              this.report(new Error(`Batch rejected: ${response.status}`));
              break;
            }
            throw new Error(`Batch export failed: ${response.status}`);
          } catch (error) {
            if (attempt + 1 === this.config.maxAttempts) this.report(error);
            else
              await new Promise((resolve) =>
                setTimeout(resolve, this.config.retryDelayMs * 2 ** attempt),
              );
          }
        }
        if (!sent) return; // Keep bounded queue and durable files for the next flush.
        if (this.config.spoolDirectory)
          for (const run of batch)
            await unlink(this.filename(run)).catch((error) =>
              this.report(error),
            );
        this.queue.splice(0, batch.length);
        this.stats.exported += batch.length;
      }
    } catch (error) {
      this.report(error);
    }
  }
  async shutdown(): Promise<void> {
    this.closed = true;
    clearInterval(this.timer);
    await this.flush();
  }
}
