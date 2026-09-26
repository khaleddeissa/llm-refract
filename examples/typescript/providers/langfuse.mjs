// Explicitly export an existing recording; no model calls are made by this script.
import { readFile } from "node:fs/promises";
import {
  exportLangfuse,
  unpack,
} from "../../../packages/typescript/dist/index.js";

if (process.argv.includes("--help")) {
  console.log(
    "Set LANGFUSE_BASE_URL, LANGFUSE_PUBLIC_KEY, LANGFUSE_SECRET_KEY; run langfuse.mjs path/to/recording.rfr",
  );
  process.exit(0);
}
const path = process.argv[2];
const baseUrl = process.env.LANGFUSE_BASE_URL;
const publicKey = process.env.LANGFUSE_PUBLIC_KEY;
const secretKey = process.env.LANGFUSE_SECRET_KEY;
if (!path || !baseUrl || !publicKey || !secretKey)
  throw new Error(
    "Provide an artifact path and LANGFUSE_BASE_URL, LANGFUSE_PUBLIC_KEY, LANGFUSE_SECRET_KEY",
  );
await exportLangfuse(unpack(await readFile(path)), baseUrl, {
  publicKey,
  secretKey,
});
console.log("Exported completed recording to Langfuse");
