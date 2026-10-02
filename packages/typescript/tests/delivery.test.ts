import { createHmac } from "node:crypto";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "vitest";
import { openWebhookInbox } from "../src/index.js";
const secret = "local-contract-only";
const scope = { organization: "org", project: "project", environment: "test" };
function message(
  id = "first",
  version = 1,
  operation = "put",
): [Buffer, string, string] {
  const body = Buffer.from(
    JSON.stringify({
      id,
      version,
      operation,
      scope,
      run_id: "run",
      payload: "stored",
    }),
  );
  return [
    body,
    `sha256=${createHmac("sha256", secret).update(body).digest("hex")}`,
    id,
  ];
}
test("inbox atomically deduplicates and fences delayed deliveries across restarts", async () => {
  const directory = await mkdtemp(join(tmpdir(), "refract-inbox-"));
  let inbox = await openWebhookInbox(join(directory, "inbox.db"), secret);
  try {
    expect(inbox.accept(...message())).toBe("applied");
    inbox.close();
    inbox = await openWebhookInbox(join(directory, "inbox.db"), secret);
    expect(inbox.accept(...message())).toBe("duplicate");
    expect(inbox.get("run", scope)).toBe("stored");
    expect(inbox.get("run", { ...scope, project: "other" })).toBeUndefined();
    expect(inbox.accept(...message("delete", 3, "delete"))).toBe("applied");
    expect(inbox.accept(...message("delayed", 2))).toBe("stale");
    expect(inbox.get("run", scope)).toBeUndefined();
    expect(() => inbox.accept(...message("first", 4))).toThrow(
      "different content",
    );
    const [body, signature, id] = message();
    expect(() =>
      inbox.accept(Buffer.concat([body, Buffer.from(" ")]), signature, id),
    ).toThrow("signature");
    expect(inbox.accept(...message("next", 4))).toBe("applied");
  } finally {
    inbox.close();
    await rm(directory, { recursive: true, force: true });
  }
});
