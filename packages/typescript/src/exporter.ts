import {
  mkdir,
  readFile,
  readdir,
  rename,
  unlink,
  open,
  lstat,
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
  /** fsync file + directory before export resolves; persistent POSIX volumes only. */
  durable?: boolean;
  maxQueueBytes?: number;
  maxSpoolBytes?: number;
  onError?: (error: unknown) => void;
}
/** Bounded background ingestion. Call shutdown() before terminating the process. */
export class BatchExporter {
  private queue: Execution[] = [];
  private flushing?: Promise<void>;
  private pending = new Set<Promise<void>>();
  private acceptance: Promise<void> = Promise.resolve();
  private initialized?: Promise<void>;
  private timer: ReturnType<typeof setInterval>;
  private closed = false;
  private reserved = 0;
  private reservedBytes = 0;
  private queuedBytes = 0;
  private spoolBytes = 0;
  readonly stats = { accepted: 0, exported: 0, dropped: 0, failures: 0 };
  private readonly config: Required<
    Omit<BatchExporterOptions, "apiKey" | "spoolDirectory" | "onError">
  > &
    BatchExporterOptions;
  constructor(options: BatchExporterOptions) {
    this.config = {
      batchSize: 32,
      durable: options.spoolDirectory !== undefined,
      maxQueueBytes: 16 * 1024 * 1024,
      maxSpoolBytes: 64 * 1024 * 1024,
      maxQueueSize: 1024,
      flushIntervalMs: 1000,
      maxAttempts: 3,
      retryDelayMs: 100,
      timeoutMs: 10_000,
      ...options,
    };
    for (const key of [
      "batchSize",
      "maxQueueBytes",
      "maxSpoolBytes",
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
    if (
      this.config.durable &&
      (!this.config.spoolDirectory || process.platform === "win32")
    )
      throw new Error(
        "durable acceptance requires a dedicated POSIX spoolDirectory",
      );
    const endpoint = new URL(options.endpoint);
    if (
      !["http:", "https:"].includes(endpoint.protocol) ||
      endpoint.username ||
      endpoint.password ||
      endpoint.search ||
      endpoint.hash
    )
      throw new Error(
        "endpoint must use HTTP(S) without credentials, query or fragment",
      );
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
      await this.recover(true);
    })());
  }
  private async recover(countBytes = false): Promise<void> {
    if (!this.config.spoolDirectory) return;
    const files: { filename: string; size: number; modified: number }[] = [];
    for (const filename of await readdir(this.config.spoolDirectory)) {
      if (!/^[a-f0-9]{64}\.json$/.test(filename)) continue;
      const metadata = await lstat(join(this.config.spoolDirectory, filename));
      if (!metadata.isFile() || metadata.isSymbolicLink()) continue;
      if (countBytes) this.spoolBytes += metadata.size;
      files.push({ filename, size: metadata.size, modified: metadata.mtimeMs });
    }
    files.sort(
      (a, b) => a.modified - b.modified || a.filename.localeCompare(b.filename),
    );
    for (const file of files) {
      if (this.queue.length >= this.config.maxQueueSize) break;
      if (
        this.queue.some(
          (run) =>
            this.filename(run) ===
            join(this.config.spoolDirectory!, file.filename),
        )
      )
        continue;
      if (file.size + this.queuedBytes > this.config.maxQueueBytes) continue;
      try {
        if (file.size > this.config.maxSpoolBytes)
          throw new Error("Spool entry exceeds capacity");
        const run = JSON.parse(
          await readFile(
            join(this.config.spoolDirectory, file.filename),
            "utf8",
          ),
        ) as Execution;
        if (
          run.spec_version !== "refract.execution.v1" ||
          !Array.isArray(run.events) ||
          typeof run.id !== "string" ||
          this.filename(run) !== join(this.config.spoolDirectory, file.filename)
        )
          throw new Error("Invalid spool execution");
        this.queue.push(run);
        this.queuedBytes += Buffer.byteLength(JSON.stringify(run));
      } catch (error) {
        this.report(error);
      }
    }
  }

  export(execution: Execution): Promise<void> {
    if (this.closed) {
      this.stats.dropped++;
      return this.config.durable
        ? Promise.reject(new Error("exporter is closed"))
        : Promise.resolve();
    }
    this.reserved++;
    const previous = this.acceptance;
    const work = (async () => {
      let reservation = 0;
      try {
        await previous;
        await this.initialize();
        if (this.queue.length + this.reserved > this.config.maxQueueSize) {
          if (this.config.durable)
            throw new Error("export queue capacity exceeded");
          this.stats.dropped++;
          return;
        }
        const run = redact(
          structuredClone(execution) as unknown as Json,
        ) as unknown as Execution;
        const serialized = JSON.stringify(run);
        const duplicate = this.queue.find((existing) => existing.id === run.id);
        if (duplicate) {
          if (JSON.stringify(duplicate) !== serialized)
            throw new Error("run ID already queued with different content");
          return;
        }
        if (this.config.spoolDirectory) {
          const existing = await readFile(this.filename(run), "utf8").catch(
            (error: NodeJS.ErrnoException) => {
              if (error.code === "ENOENT") return undefined;
              throw error;
            },
          );
          if (existing !== undefined) {
            if (existing !== serialized)
              throw new Error("run ID already spooled with different content");
            return;
          }
        }
        const size = Buffer.byteLength(serialized);
        if (
          size + this.queuedBytes + this.reservedBytes >
            this.config.maxQueueBytes ||
          (this.config.spoolDirectory &&
            size + this.spoolBytes + this.reservedBytes >
              this.config.maxSpoolBytes)
        )
          throw new Error("export byte capacity exceeded");
        reservation = size;
        this.reservedBytes += size;
        if (this.config.spoolDirectory) {
          const filename = this.filename(run);
          const temporary = `${filename}.${randomUUID()}.tmp`;
          const file = await open(temporary, "wx", 0o600);
          try {
            await file.writeFile(serialized);
            if (this.config.durable) await file.sync();
          } finally {
            await file.close();
          }
          await rename(temporary, filename);
          if (this.config.durable) {
            const directory = await open(this.config.spoolDirectory, "r");
            try {
              await directory.sync();
            } finally {
              await directory.close();
            }
          }
          this.spoolBytes += size;
        }
        this.queue.push(run);
        this.queuedBytes += size;
        this.stats.accepted++;
      } catch (error) {
        this.stats.dropped++;
        this.report(error);
        if (this.config.durable) throw error;
      } finally {
        this.reservedBytes -= reservation;
        this.reserved--;
      }
    })();
    this.acceptance = work.catch(() => {});
    this.pending.add(work);
    void work.then(
      () => this.pending.delete(work),
      () => this.pending.delete(work),
    );
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
                redirect: "error",
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
            await response.body?.cancel();
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
        if (this.config.spoolDirectory) {
          for (const run of batch) await unlink(this.filename(run));
          if (this.config.durable) {
            const directory = await open(this.config.spoolDirectory, "r");
            try {
              await directory.sync();
            } finally {
              await directory.close();
            }
          }
        }
        const bytes = batch.reduce(
          (sum, run) => sum + Buffer.byteLength(JSON.stringify(run)),
          0,
        );
        this.queuedBytes -= bytes;
        if (this.config.spoolDirectory)
          this.spoolBytes = Math.max(0, this.spoolBytes - bytes);
        this.queue.splice(0, batch.length);
        this.stats.exported += batch.length;
        await this.recover();
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
