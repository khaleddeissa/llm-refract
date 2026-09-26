import { readFileSync, writeFileSync } from "node:fs";
import assert from "node:assert/strict";
import {
  fromOtlp,
  toOtlp,
  pack,
} from "../../packages/typescript/dist/index.js";
const [run] = fromOtlp(JSON.parse(readFileSync(process.argv[2], "utf8")));
assert.equal(run.events[1].attributes.input_tokens, 40);
assert.equal(run.events[1].attributes.total_tokens, 52);
assert.equal(run.events[1].attributes.api_key, "[REDACTED]");
writeFileSync(process.argv[3], pack(run));
writeFileSync(process.argv[4], JSON.stringify(toOtlp(run)));
