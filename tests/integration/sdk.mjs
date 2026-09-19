import assert from "node:assert/strict";
import { refract, pack, unpack } from "../../packages/typescript/dist/index.js";
const endpoint = process.env.REFRACT_SERVER_URL ?? "http://127.0.0.1:8000";
let snapshot;
await refract.run(
  "node-http-integration",
  async () => {
    refract.event({
      type: "generation",
      name: "answer",
      input: { api_key: "do-not-store" },
      output: { text: "hello" },
    });
  },
  {
    endpoint,
    onComplete: (r) => {
      snapshot = r;
    },
  },
);
const response = await fetch(`${endpoint}/v1/runs/${snapshot.id}`);
assert.equal(response.status, 200);
const stored = await response.json();
assert.equal(stored.events[0].input.api_key, "[REDACTED]");
assert.deepEqual(unpack(pack(stored)), stored);
const exported = await fetch(`${endpoint}/v1/runs/${snapshot.id}/artifact`);
assert.equal(
  unpack(new Uint8Array(await exported.arrayBuffer())).id,
  snapshot.id,
);
console.log("Node SDK → API → text artifact passed");
