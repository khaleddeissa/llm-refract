import { readFileSync, writeFileSync } from "node:fs";
import { pack, unpack } from "../../packages/typescript/dist/index.js";
writeFileSync(process.argv[3], pack(unpack(readFileSync(process.argv[2]))), {
  flag: "wx",
});
