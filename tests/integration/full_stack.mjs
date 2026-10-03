import assert from "node:assert/strict";
import {
  RefractClient,
  refract,
  unpack,
} from "../../packages/typescript/dist/index.js";
const client = new RefractClient(
  process.env.REFRACT_SERVER_URL,
  process.env.REFRACT_API_KEY,
);
let snapshot;
await refract.run(
  "Node support lookup",
  async () => {
    refract.event({
      type: "tool.call",
      name: "Lookup order",
      output: { found: true },
    });
  },
  {
    onComplete: (r) => {
      snapshot = r;
    },
  },
);
const stored = await client.request("/v1/runs", snapshot);
assert.equal(stored.id, snapshot.id);
assert.ok((await client.telemetry("logs")).records.length);
assert.ok((await client.searchText("return policy")).runs.length);
const response = await fetch(
  `${process.env.REFRACT_SERVER_URL}/v1/runs/${stored.id}/artifact`,
  {
    headers: { Authorization: `Bearer ${process.env.REFRACT_API_KEY}` },
  },
);
assert.equal(
  unpack(new Uint8Array(await response.arrayBuffer())).id,
  stored.id,
);
console.log(
  "Node SDK authenticated capture, text search, telemetry and artifact passed",
);
