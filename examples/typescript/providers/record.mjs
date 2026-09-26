// Offline fixtures for every supported provider contract. No credentials or inference requests.
import { mkdir } from "node:fs/promises";
import {
  refract,
  instrumentOpenAI,
  instrumentAnthropic,
  instrumentAzureOpenAI,
  instrumentGemini,
  instrumentVertex,
  instrumentBedrock,
  instrumentCustom,
} from "../../../packages/typescript/dist/index.js";
await mkdir(".examples", { recursive: true });
const openai = {
  responses: {
    async create({ model }) {
      return {
        model,
        output_text: "OpenAI fixture",
        usage: { input_tokens: 8, output_tokens: 2 },
      };
    },
  },
};
const azure = {
  chat: {
    completions: {
      async create({ model }) {
        return {
          model,
          choices: [{ message: { content: "Azure fixture" } }],
          usage: { prompt_tokens: 8, completion_tokens: 2 },
        };
      },
    },
  },
};
const anthropic = {
  messages: {
    async create({ model }) {
      return {
        model,
        content: [{ type: "text", text: "Anthropic fixture" }],
        usage: {
          input_tokens: 6,
          output_tokens: 2,
          cache_read_input_tokens: 2,
        },
      };
    },
  },
};
function google() {
  return {
    models: {
      async generateContentStream() {
        return (async function* () {
          yield {
            candidates: [{ content: { parts: [{ text: "Google fixture" }] } }],
          };
          yield {
            usageMetadata: {
              promptTokenCount: 8,
              candidatesTokenCount: 2,
              totalTokenCount: 10,
            },
          };
        })();
      },
    },
  };
}
const gemini = google();
const vertex = google();
class ConverseStreamCommand {
  constructor(input) {
    this.input = input;
  }
}
const bedrock = {
  async send() {
    return {
      stream: (async function* () {
        yield { contentBlockDelta: { delta: { text: "Bedrock fixture" } } };
        yield {
          metadata: {
            usage: { inputTokens: 8, outputTokens: 2, totalTokens: 10 },
          },
        };
      })(),
    };
  },
};
const local = {
  async generate() {
    return { answer: "Local fixture", generated: 2 };
  },
};
const restores = [
  instrumentOpenAI(openai),
  instrumentAzureOpenAI(azure),
  instrumentAnthropic(anthropic),
  instrumentGemini(gemini),
  instrumentVertex(vertex),
  instrumentBedrock(bedrock),
  instrumentCustom(local, {
    provider: "custom",
    methods: [["generate"]],
    request: () => ({ model: "local-fixture", input: "demo" }),
    normalize: (value) => ({
      output: value.answer,
      usage: { output_tokens: value.generated },
    }),
  }),
];
const path = `.examples/providers-${Date.now()}.rfr`;
try {
  await refract.run(
    "provider-contracts",
    async () => {
      await openai.responses.create({ model: "openai-fixture", input: "demo" });
      await azure.chat.completions.create({
        model: "azure-fixture",
        messages: [],
      });
      await anthropic.messages.create({
        model: "anthropic-fixture",
        messages: [],
      });
      for (const client of [gemini, vertex]) {
        for await (const _chunk of await client.models.generateContentStream({
          model: "google-fixture",
          contents: "demo",
        })) {
          /* consume */
        }
      }
      const response = await bedrock.send(
        new ConverseStreamCommand({ modelId: "bedrock-fixture", messages: [] }),
      );
      for await (const _chunk of response.stream) {
        /* consume */
      }
      await local.generate();
    },
    { path, failOpen: false },
  );
  console.log(`Recorded seven provider contracts in ${path}`);
} finally {
  restores.forEach((restore) => restore());
}
