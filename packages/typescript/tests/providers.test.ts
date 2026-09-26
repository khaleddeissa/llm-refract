import { expect, it } from "vitest";
import {
  instrumentAzureOpenAI,
  instrumentGemini,
  instrumentVertex,
  instrumentBedrock,
  instrumentCustom,
  refract,
  redact,
  type Execution,
  type Json,
} from "../src/index.js";

async function record(
  fn: () => unknown | Promise<unknown>,
): Promise<Execution> {
  let run!: Execution;
  await refract.run("provider-contract", fn, {
    onComplete: (value) => {
      run = value;
    },
  });
  return run;
}

it.each([instrumentGemini, instrumentVertex])(
  "captures Google content, functions and final usage with %s",
  async (instrument) => {
    const response: Json = {
      modelVersion: "gemini-fixture",
      candidates: [
        {
          content: {
            parts: [
              { text: "hello" },
              { functionCall: { name: "lookup", args: { id: 1 } } },
            ],
          },
        },
      ],
      usageMetadata: {
        promptTokenCount: 30,
        candidatesTokenCount: 8,
        thoughtsTokenCount: 2,
        cachedContentTokenCount: 10,
        totalTokenCount: 40,
      },
    };
    const client = {
      models: { generateContent: async (_params: unknown) => response },
    };
    const original = client.models.generateContent;
    const restore = instrument(client, {
      pricing: { "gemini-fixture": { input: 1, output: 3, cachedInput: 0.5 } },
    });
    const run = await record(async () => {
      expect(
        await client.models.generateContent({
          model: "gemini-fixture",
          contents: "hello",
        }),
      ).toBe(response);
    });
    expect(run.events[0].attributes).toMatchObject({
      input_tokens: 30,
      output_tokens: 10,
      cache_read_tokens: 10,
      total_tokens: 40,
      cost_usd: 0.000055,
    });
    expect(run.events[0].output).toEqual(redact(response));
    restore();
    expect(client.models.generateContent).toBe(original);
  },
);

it("captures Google streaming text and function calls while excluding thought deltas from visible text", async () => {
  const chunks = [
    {
      candidates: [
        { content: { parts: [{ thought: true, text: "private" }] } },
      ],
    },
    { candidates: [{ content: { parts: [{ text: "Hi" }] } }] },
    {
      candidates: [
        {
          content: {
            parts: [
              { functionCall: { name: "search", args: { query: "hello" } } },
            ],
          },
        },
      ],
      usageMetadata: {
        promptTokenCount: 10,
        candidatesTokenCount: 2,
        totalTokenCount: 12,
      },
    },
  ];
  const client = {
    models: {
      generateContentStream: async (_params: unknown) =>
        (async function* () {
          yield* chunks;
        })(),
    },
  };
  instrumentGemini(client);
  const run = await record(async () => {
    const observed = [];
    for await (const chunk of await client.models.generateContentStream({
      model: "gemini-fixture",
    }))
      observed.push(chunk);
    observed.forEach((chunk, index) => expect(chunk).toBe(chunks[index]));
  });
  expect(run.events[0].output).toEqual({
    text: "Hi",
    tool_calls: [{ name: "search", args: { query: "hello" } }],
  });
  expect(run.events[0].attributes).toMatchObject({
    provider: "gemini",
    total_tokens: 12,
    stream_completed: true,
  });
  expect(run.events[0].attributes?.ttft_ms).toBeGreaterThanOrEqual(0);
});

class ConverseCommand {
  constructor(readonly input: Record<string, unknown>) {}
}
class ConverseStreamCommand {
  constructor(readonly input: Record<string, unknown>) {}
}
class InvokeModelCommand {
  constructor(readonly input: Record<string, unknown>) {}
}

it("instruments Bedrock Converse envelopes and preserves AWS metadata, client binding, arguments and unrelated methods", async () => {
  const response = {
    output: { message: { role: "assistant", content: [{ text: "hello" }] } },
    usage: {
      inputTokens: 30,
      outputTokens: 5,
      totalTokens: 35,
      cacheReadInputTokens: 10,
    },
    $metadata: { requestId: "request-id" },
  };
  const options = { abortSignal: new AbortController().signal };
  const command = new ConverseCommand({
    modelId: "bedrock-model",
    messages: [],
  });
  const client = {
    marker: "unchanged",
    async send(received: ConverseCommand, config?: unknown) {
      expect(this.marker).toBe("unchanged");
      expect(received).toBe(command);
      expect(config).toBe(options);
      return response;
    },
  };
  instrumentBedrock(client);
  const run = await record(async () => {
    expect(await client.send(command, options)).toBe(response);
  });
  expect(run.events[0].attributes).toMatchObject({
    provider: "bedrock",
    model: "bedrock-model",
    total_tokens: 35,
    cache_read_tokens: 10,
  });
  expect(run.events[0].input).toEqual(command.input);
  expect(run.events[0].output).toEqual(redact(response));
});

it("captures Bedrock ConverseStream nested iteration, usage and exception events without altering the envelope", async () => {
  const chunks = [
    {
      contentBlockStart: {
        start: { toolUse: { toolUseId: "call-1", name: "search" } },
      },
    },
    { contentBlockDelta: { delta: { text: "Hello" } } },
    {
      metadata: {
        usage: { inputTokens: 20, outputTokens: 2, totalTokens: 22 },
      },
    },
    { modelStreamErrorException: { message: "upstream stopped" } },
  ];
  const metadata = { requestId: "stream-id" };
  const client = {
    send: async (_command: ConverseStreamCommand) => ({
      stream: (async function* () {
        yield* chunks;
      })(),
      $metadata: metadata,
    }),
  };
  instrumentBedrock(client);
  const run = await record(async () => {
    const response = await client.send(
      new ConverseStreamCommand({ modelId: "fixture" }),
    );
    expect(response.$metadata).toBe(metadata);
    const observed = [];
    for await (const chunk of response.stream) observed.push(chunk);
    expect(observed).toEqual(chunks);
  });
  expect(run.events[0].status).toBe("failed");
  expect(run.events[0].output).toMatchObject({
    text: "Hello",
    tool_calls: [{ toolUseId: "call-1", name: "search" }],
  });
  expect(run.events[0].attributes).toMatchObject({
    stream_completed: false,
    total_tokens: 22,
  });
});

it("does not retry or record unsupported Bedrock commands and callback calls", async () => {
  const failure = new Error("once only");
  let count = 0;
  const client = {
    send: (_command: unknown, _callback?: () => void) => {
      count++;
      throw failure;
    },
  };
  instrumentBedrock(client);
  const run = await record(() => {
    expect(() =>
      client.send(new InvokeModelCommand({ modelId: "custom" })),
    ).toThrow(failure);
    expect(() =>
      client.send(new ConverseCommand({ modelId: "custom" }), () => {}),
    ).toThrow(failure);
  });
  expect(count).toBe(2);
  expect(run.events).toHaveLength(0);
});

it("distinguishes Azure and captures compatible streaming usage", async () => {
  const client = {
    chat: {
      completions: {
        create: async (_params: unknown) => ({
          model: "deployment",
          usage: { prompt_tokens: 4, completion_tokens: 2, total_tokens: 6 },
        }),
      },
    },
  };
  instrumentAzureOpenAI(client);
  const run = await record(() =>
    client.chat.completions.create({ model: "deployment", messages: [] }),
  );
  expect(run.events[0].attributes).toMatchObject({
    provider: "azure",
    model: "deployment",
    input_tokens: 4,
    output_tokens: 2,
    total_tokens: 6,
  });
});

it("adapts custom local model inputs and responses without changing the caller contract", async () => {
  const answer = { answer: "local", used: 6 };
  const client = {
    async generate(_prompt: string, _model: string) {
      return answer;
    },
  };
  instrumentCustom(client, {
    provider: "my-local-engine",
    methods: [["generate"]],
    request: (args) => ({
      model: String(args[1]),
      input: { prompt: String(args[0]) },
    }),
    normalize: (value) => {
      const response = value as typeof answer;
      return {
        output: response.answer,
        usage: { output_tokens: response.used },
      };
    },
  });
  const run = await record(async () => {
    expect(await client.generate("hello", "local-model")).toBe(answer);
  });
  expect(run.events[0].input).toEqual({ prompt: "hello" });
  expect(run.events[0].output).toBe("local");
  expect(run.events[0].attributes).toMatchObject({
    provider: "my-local-engine",
    model: "local-model",
    output_tokens: 6,
  });
});

it("custom normalization failures do not mask provider results or stream chunks", async () => {
  const client = {
    async generate() {
      return (async function* () {
        yield "first";
        yield "second";
      })();
    },
  };
  instrumentCustom(client, {
    provider: "custom",
    methods: [["generate"]],
    normalize: () => {
      throw new Error("unsupported capture shape");
    },
  });
  const run = await record(async () => {
    const chunks = [];
    for await (const chunk of await client.generate()) chunks.push(chunk);
    expect(chunks).toEqual(["first", "second"]);
  });
  expect(run.events[0].attributes).toMatchObject({
    capture_incomplete: true,
    stream_completed: true,
  });
  expect(run.events[0].status).toBe("completed");
});

it.each(["__proto__", "prototype", "constructor"])(
  "rejects unsafe %s segments before patching any methods",
  (segment) => {
    for (const path of [
      [segment],
      [segment, "toString"],
      ["nested", segment],
    ]) {
      const original = () => "unchanged";
      const client = { generate: original, nested: {} };
      const before = Object.getOwnPropertyDescriptors(Object.prototype);
      expect(() =>
        instrumentCustom(client, {
          provider: "custom",
          methods: [["generate"], path],
        }),
      ).toThrow("Unsafe instrumentation property path");
      expect(client.generate).toBe(original);
      expect(Object.getOwnPropertyDescriptors(Object.prototype)).toEqual(
        before,
      );
    }
  },
);

it("rejects prototype objects reached through ordinary aliases", () => {
  const original = () => "unchanged";
  const client = { generate: original, shared: Object.prototype };
  expect(() =>
    instrumentCustom(client, {
      provider: "custom",
      methods: [["generate"], ["shared", "toString"]],
    }),
  ).toThrow("Cannot instrument a prototype object");
  expect(client.generate).toBe(original);
});

it("restores inherited methods without leaving an own property", async () => {
  class Client {
    generate() {
      return "result";
    }
  }
  const client = new Client();
  const original = Client.prototype.generate;
  const restore = instrumentCustom(client, {
    provider: "custom",
    methods: [["generate"]],
  });
  expect(Object.hasOwn(client, "generate")).toBe(true);
  await record(() => expect(client.generate()).toBe("result"));
  expect(Client.prototype.generate).toBe(original);
  restore();
  restore();
  expect(Object.hasOwn(client, "generate")).toBe(false);
  expect(client.generate).toBe(original);
});

it("preserves a method's own property descriptor when restored", () => {
  const client = {};
  Object.defineProperty(client, "generate", {
    value: () => "result",
    writable: true,
    enumerable: false,
    configurable: false,
  });
  const descriptor = Object.getOwnPropertyDescriptor(client, "generate");
  const restore = instrumentCustom(client, {
    provider: "custom",
    methods: [["generate"]],
  });
  restore();
  expect(Object.getOwnPropertyDescriptor(client, "generate")).toEqual(
    descriptor,
  );
});
