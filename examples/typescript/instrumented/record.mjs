// Offline provider-shaped fixture. No external SDK, API key, or paid request is needed.
// For real applications pass new OpenAI() / new Anthropic() into these same wrappers.
import { mkdir } from "node:fs/promises";
import {
  instrumentOpenAI,
  instrumentAnthropic,
  refract,
  BatchExporter,
} from "../../../packages/typescript/dist/index.js";
await mkdir(".examples", { recursive: true });
const openai = {
  responses: {
    async create({ model }) {
      return {
        model,
        output_text: "A portable execution you can inspect and compare.",
        usage: {
          input_tokens: 80,
          output_tokens: 12,
          input_tokens_details: { cached_tokens: 20 },
        },
      };
    },
  },
};
const anthropic = {
  messages: {
    async create() {
      return (async function* () {
        yield {
          type: "message_start",
          message: { usage: { input_tokens: 30, cache_read_input_tokens: 10 } },
        };
        yield { type: "content_block_delta", delta: { text: "Reviewed." } };
        yield { type: "message_delta", usage: { output_tokens: 3 } };
      })();
    },
  },
};
const restoreOpenAI = instrumentOpenAI(openai, {
  pricing: { "fixture-model": { input: 1, output: 3, cachedInput: 0.1 } },
});
const restoreAnthropic = instrumentAnthropic(anthropic);
const exporter = process.env.REFRACT_ENDPOINT
  ? new BatchExporter({
      endpoint: process.env.REFRACT_ENDPOINT,
      apiKey: process.env.REFRACT_API_KEY,
      spoolDirectory: ".examples/spool",
    })
  : undefined;
try {
  await refract.run(
    "instrumented-agent",
    () =>
      refract.span(
        { type: "tool.call", name: "orchestrate", replay_policy: "MOCK" },
        async () => {
          await openai.responses.create({
            model: "fixture-model",
            input: "Describe Refract",
          });
          for await (const chunk of await anthropic.messages.create({
            model: "fixture-reviewer",
            stream: true,
          })) {
            if (chunk.delta?.text) console.log(chunk.delta.text);
          }
        },
      ),
    {
      path: `.examples/instrumented-${Date.now()}.rfr`,
      exporter,
      failOpen: false,
    },
  );
} finally {
  restoreOpenAI();
  restoreAnthropic();
  await exporter?.shutdown();
}
