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

it("does not retry Bedrock calls and skips unsupported commands", async () => {
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
  expect(run.events).toHaveLength(1);
  expect(run.events[0].status).toBe("failed");
});

it("observes callback Bedrock responses and preserves callback context and errors", async () => {
  const failure = new Error("fixture callback failure");
  const client = {
    send(
      command: ConverseCommand,
      callback: (error: Error | null, result?: unknown) => void,
    ) {
      queueMicrotask(() =>
        callback(
          command.input.modelId === "bad" ? failure : null,
          command.input.modelId === "bad"
            ? undefined
            : {
                usage: { inputTokens: 3, outputTokens: 2 },
                output: { message: { content: [{ text: "answer" }] } },
              },
        ),
      );
    },
  };
  const restore = instrumentBedrock(client);
  try {
    const run = await record(async () => {
      await new Promise<void>((resolve, reject) =>
        client.send(
          new ConverseCommand({ modelId: "good" }),
          (error, result) => {
            if (error) {
              reject(error);
              return;
            }
            expect(result).toMatchObject({ usage: { inputTokens: 3 } });
            client.send(new ConverseCommand({ modelId: "bad" }), (error) => {
              expect(error).toBe(failure);
              resolve();
            });
          },
        ),
      );
    });
    expect(run.events).toHaveLength(2);
    expect(run.events[0].attributes?.total_tokens).toBe(5);
    expect(run.events[1].status).toBe("failed");
  } finally {
    restore();
  }
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

it("records native Bedrock bytes and preserves unknown usage", async () => {
  const { instrumentBedrockNative } = await import("../src/index.js");
  class InvokeModelCommand {
    constructor(public input: object) {}
  }
  const result = {
    body: new TextEncoder().encode(
      JSON.stringify({ generation: "ok", generation_token_count: 4 }),
    ),
  };
  const client = { send: async (_command: InvokeModelCommand) => result };
  const restore = instrumentBedrockNative(client);
  const captured = await record(async () => {
    expect(
      await client.send(
        new InvokeModelCommand({ modelId: "local", body: '{"prompt":"hi"}' }),
      ),
    ).toBe(result);
  });
  expect(captured.events[0].attributes?.output_tokens).toBe(4);
  expect(captured.events[0].attributes?.input_tokens).toBeUndefined();
  expect(captured.events[0].attributes?.total_tokens).toBeUndefined();
  restore();
});
it("observes Ollama usage without changing local model results", async () => {
  const { instrumentLibrary } = await import("../src/index.js");
  const result = {
    message: { content: "local" },
    prompt_eval_count: 3,
    eval_count: 2,
  };
  const client = { chat: async (_input: object) => result };
  const restore = instrumentLibrary(client, "ollama");
  const captured = await record(async () => {
    expect(await client.chat({ model: "local", messages: [] })).toBe(result);
  });
  expect(captured.events[0].attributes?.total_tokens).toBe(5);
  restore();
});

it("merges native Bedrock usage across streaming messages", async () => {
  const { instrumentBedrockNative } = await import("../src/index.js");
  class InvokeModelWithResponseStreamCommand {
    constructor(public input: object) {}
  }
  const messages = [
    { type: "message_start", message: { usage: { input_tokens: 9 } } },
    { type: "content_block_delta", delta: { text: "hi" } },
    { type: "message_delta", usage: { output_tokens: 2 } },
  ];
  const chunks = messages.map((message) => ({
    chunk: { bytes: new TextEncoder().encode(JSON.stringify(message)) },
  }));
  const client = {
    send: async (_command: object) => ({
      body: (async function* () {
        yield* chunks;
      })(),
    }),
  };
  const restore = instrumentBedrockNative(client);
  const captured = await record(async () => {
    const result = await client.send(
      new InvokeModelWithResponseStreamCommand({
        modelId: "fixture",
        body: "{}",
      }),
    );
    const observed = [];
    for await (const chunk of result.body) observed.push(chunk);
    expect(observed).toEqual(chunks);
  });
  expect(captured.events[0].attributes).toMatchObject({
    input_tokens: 9,
    output_tokens: 2,
    total_tokens: 11,
  });
  expect(captured.events[0].output).toEqual({ text: "hi", tool_calls: [] });
  restore();
});

it("records Realtime responses in the installation context and bounds partial output", async () => {
  const { EventEmitter } = await import("node:events");
  const { AsyncResource } = await import("node:async_hooks");
  const { instrumentRealtime } = await import("../src/index.js");
  const socketContext = new AsyncResource("fixture-socket");
  const emitter = new EventEmitter();
  const emit = (value: object) =>
    socketContext.runInAsyncScope(() => emitter.emit("event", value));
  const captured = await record(() => {
    const stop = instrumentRealtime(emitter, {
      provider: "openai",
      model: "local",
      maxOutputChars: 3,
    });
    emit({ type: "response.created", response: { id: "one" } });
    emit({
      type: "response.output_audio.delta",
      response_id: "one",
      delta: "private-audio",
    });
    emit({
      type: "response.output_text.delta",
      response_id: "one",
      delta: "hello",
    });
    emit({
      type: "response.done",
      response: {
        id: "one",
        status: "completed",
        usage: { input_tokens: 3, output_tokens: 2 },
      },
    });
    emit({ type: "response.created", response: { id: "two" } });
    stop();
    stop();
    expect(emitter.listenerCount("event")).toBe(0);
  });
  expect(captured.events).toHaveLength(2);
  expect(captured.events[0].output).toEqual({ text: "hel" });
  expect(captured.events[0].attributes).toMatchObject({
    total_tokens: 5,
    output_truncated: true,
    stream_completed: true,
  });
  expect(captured.events[1].attributes).toMatchObject({
    capture_incomplete: true,
    stream_completed: false,
  });
  expect(JSON.stringify(captured)).not.toContain("private-audio");
});
