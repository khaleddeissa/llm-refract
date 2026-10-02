import { createHash, createHmac, timingSafeEqual } from "node:crypto";
import { open } from "node:fs/promises";
import type { DatabaseSync } from "node:sqlite";

export interface DeliveryMessage {
  id: string;
  version: number;
  scope: { organization: string; project: string; environment: string };
  run_id: string;
  operation: "put" | "delete";
  payload: string | null;
}
/** Verify the original request bytes, never reserialized JSON. */
export function verifyDelivery(
  body: Uint8Array,
  signature: string,
  secret: string,
  deliveryId: string,
): DeliveryMessage {
  if (!secret || body.byteLength > 17 * 1024 * 1024)
    throw new Error("Missing secret or oversized delivery");
  const expected = Buffer.from(
    `sha256=${createHmac("sha256", secret).update(body).digest("hex")}`,
  );
  const supplied = Buffer.from(signature);
  if (
    expected.length !== supplied.length ||
    !timingSafeEqual(expected, supplied)
  )
    throw new Error("Invalid webhook signature");
  const message = JSON.parse(
    Buffer.from(body).toString("utf8"),
  ) as DeliveryMessage;
  if (!message || typeof message !== "object" || message.id !== deliveryId)
    throw new Error("Delivery ID mismatch");
  for (const value of [message.id, message.run_id])
    if (typeof value !== "string" || !value.length || value.length > 512)
      throw new Error("Invalid delivery identity");
  if (
    !message.scope ||
    Object.keys(message.scope).sort().join(",") !==
      "environment,organization,project" ||
    Object.values(message.scope).some(
      (v) => typeof v !== "string" || !v.length || v.length > 128,
    )
  )
    throw new Error("Invalid delivery scope");
  if (
    !Number.isSafeInteger(message.version) ||
    message.version < 0 ||
    !["put", "delete"].includes(message.operation)
  )
    throw new Error("Invalid delivery version or operation");
  if (message.operation === "put" && typeof message.payload !== "string")
    throw new Error("Put requires stored payload");
  return message;
}
/** Atomic local mirror. Send HTTP 2xx only after accept returns. No external effects are implied. */
export class WebhookInbox {
  constructor(
    private readonly db: DatabaseSync,
    private readonly secret: string,
  ) {
    if (!secret) throw new Error("Webhook secret is required");
    db.exec(`PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA busy_timeout=30000;
      CREATE TABLE IF NOT EXISTS receipts (id TEXT PRIMARY KEY, digest TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS deliveries (organization TEXT,project TEXT,environment TEXT,run_id TEXT,version INTEGER NOT NULL,payload TEXT,deleted INTEGER NOT NULL,PRIMARY KEY(organization,project,environment,run_id));`);
  }
  accept(
    body: Uint8Array,
    signature: string,
    deliveryId: string,
  ): "applied" | "duplicate" | "stale" {
    const message = verifyDelivery(body, signature, this.secret, deliveryId);
    const digest = createHash("sha256").update(body).digest("hex");
    const identity = [
      message.scope.organization,
      message.scope.project,
      message.scope.environment,
      message.run_id,
    ];
    this.db.exec("BEGIN IMMEDIATE");
    try {
      const receipt = this.db
        .prepare("SELECT digest FROM receipts WHERE id=?")
        .get(deliveryId);
      if (receipt) {
        if (receipt.digest !== digest)
          throw new Error("Delivery ID reused with different content");
        this.db.exec("COMMIT");
        return "duplicate";
      }
      const current = this.db
        .prepare(
          "SELECT version FROM deliveries WHERE organization=? AND project=? AND environment=? AND run_id=?",
        )
        .get(...identity);
      let result: "applied" | "stale" = "stale";
      if (!current || message.version > Number(current.version)) {
        this.db
          .prepare(
            "INSERT INTO deliveries VALUES(?,?,?,?,?,?,?) ON CONFLICT(organization,project,environment,run_id) DO UPDATE SET version=excluded.version,payload=excluded.payload,deleted=excluded.deleted",
          )
          .run(
            ...identity,
            message.version,
            message.operation === "put" ? message.payload : null,
            Number(message.operation === "delete"),
          );
        result = "applied";
      }
      this.db
        .prepare("INSERT INTO receipts VALUES(?,?)")
        .run(deliveryId, digest);
      this.db.exec("COMMIT");
      return result;
    } catch (error) {
      this.db.exec("ROLLBACK");
      throw error;
    }
  }
  get(runId: string, scope: DeliveryMessage["scope"]): string | undefined {
    return this.db
      .prepare(
        "SELECT payload FROM deliveries WHERE organization=? AND project=? AND environment=? AND run_id=? AND deleted=0",
      )
      .get(scope.organization, scope.project, scope.environment, runId)
      ?.payload as string | undefined;
  }
  close(): void {
    this.db.close();
  }
}
/** Requires Node's node:sqlite (Node 22.13+ without flags; Node 24 recommended). */
export async function openWebhookInbox(
  path: string,
  secret: string,
): Promise<WebhookInbox> {
  if (path !== ":memory:") {
    try {
      const file = await open(path, "wx", 0o600);
      await file.close();
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "EEXIST") throw error;
    }
  }
  const { DatabaseSync } = await import("node:sqlite");
  return new WebhookInbox(new DatabaseSync(path), secret);
}
