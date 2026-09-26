// Calls your existing OpenAI-compatible inference endpoint. Refract does not host the model.
import { mkdir } from "node:fs/promises";
import {
  instrumentOpenAI,
  refract,
} from "../../../packages/typescript/dist/index.js";

if (process.argv.includes("--help")) {
  console.log(
    "Set LLM_BASE_URL, LLM_MODEL and optionally LLM_API_KEY; then run this file. Install openai in your application first.",
  );
  process.exit(0);
}
if (!process.env.LLM_BASE_URL || !process.env.LLM_MODEL)
  throw new Error(
    "Set LLM_BASE_URL and LLM_MODEL to your running model server",
  );
const { default: OpenAI } = await import("openai");
const client = new OpenAI({
  baseURL: process.env.LLM_BASE_URL,
  apiKey: process.env.LLM_API_KEY ?? "local",
});
const restore = instrumentOpenAI(client);
await mkdir(".examples", { recursive: true });
const path = `.examples/local-model-${Date.now()}.rfr`;
try {
  await refract.run(
    "local-model",
    async () => {
      const chunks = await client.chat.completions.create({
        model: process.env.LLM_MODEL,
        messages: [
          {
            role: "user",
            content: "Explain portable execution traces in one sentence.",
          },
        ],
        stream: true,
        ...(process.env.LLM_INCLUDE_USAGE === "1"
          ? { stream_options: { include_usage: true } }
          : {}),
      });
      for await (const chunk of chunks)
        process.stdout.write(chunk.choices[0]?.delta.content ?? "");
      process.stdout.write("\n");
    },
    {
      path,
      endpoint: process.env.REFRACT_ENDPOINT,
      apiKey: process.env.REFRACT_API_KEY,
      failOpen: false,
    },
  );
  console.log(`Recorded ${path}`);
} finally {
  restore();
}
