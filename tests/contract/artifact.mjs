import { readFileSync } from "node:fs";
import assert from "node:assert/strict";
import { unpack } from "../../packages/typescript/dist/index.js";
const run = unpack(readFileSync(process.argv[2]));
assert.ok(run.events.every((event) => event.run_id === run.id));
console.log(
  `Node verified ${run.id}: ${run.events.length} events and checksum`,
);
