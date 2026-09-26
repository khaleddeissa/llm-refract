import { expect, it } from "vitest";
import {
  instrumentOpenAI,
  instrumentAnthropic,
  refract,
  redact,
  type Execution,
} from "../src/index.js";

it("captures provider calls, exact numeric metrics, costs and causal span parents", async () => {
  const create = async () => ({
    model: "demo",
    output_text: "answer",
    usage: {
      input_tokens: 100,
      output_tokens: 20,
      input_tokens_details: { cached_tokens: 30 },
    },
  });
  const client = { responses: { create } };
  const restore = instrumentOpenAI(client, {
    pricing: { demo: { input: 2, output: 8, cachedInput: 1 } },
  });
  let captured!: Execution;
  await refract.run(
    "agent",
    () =>
      refract.span({ type: "tool.call", name: "planner" }, async () => {
        const result = await client.responses.create();
        expect(result.output_text).toBe("answer");
      }),
    {
      onComplete: (run) => {
        captured = run;
      },
    },
  );
  expect(captured.events).toHaveLength(2);
  const event = captured.events[1];
  expect(event.parent_id).toBe(captured.events[0].id);
  expect(event.attributes).toMatchObject({
    provider: "openai",
    model: "demo",
    input_tokens: 100,
    output_tokens: 20,
    cache_read_tokens: 30,
    cost_usd: 0.00033,
  });
  expect(event.duration_ms).toBeGreaterThanOrEqual(0);
  restore();
  expect(client.responses.create).toBe(create);
  expect(
    redact({
      input_tokens: 12,
      access_token: "secret",
      input_tokens_secret: 2,
    }),
  ).toEqual({
    input_tokens: 12,
    access_token: "[REDACTED]",
    input_tokens_secret: "[REDACTED]",
  });
});
it("captures Anthropic streaming usage, TTFT and unchanged chunks", async () => {
  async function* chunks() {
    yield {
      type: "message_start",
      message: { usage: { input_tokens: 50, cache_read_input_tokens: 10 } },
    };
    yield { type: "content_block_delta", delta: { text: "Hello" } };
    yield { type: "message_delta", usage: { output_tokens: 4 } };
  }
  const client = { messages: { create: async (_params: unknown) => chunks() } };
  instrumentAnthropic(client);
  let captured!: Execution;
  await refract.run(
    "stream",
    async () => {
      const received = [];
      for await (const item of await client.messages.create({
        model: "claude-demo",
        stream: true,
      }))
        received.push(item);
      expect(received).toHaveLength(3);
    },
    {
      onComplete: (run) => {
        captured = run;
      },
    },
  );
  expect(captured.events[0].attributes).toMatchObject({
    input_tokens: 50,
    output_tokens: 4,
    cache_read_tokens: 10,
    total_tokens: 64,
    stream_completed: true,
  });
  expect(captured.events[0].attributes?.ttft_ms).toBeGreaterThanOrEqual(0);
  expect(captured.events[0].output).toMatchObject({ text: "Hello" });
});
it("preserves provider error identity and records failed generation", async () => {
  const error = new Error("provider unavailable");
  const client = {
    responses: {
      create: async () => {
        throw error;
      },
    },
  };
  instrumentOpenAI(client);
  let captured!: Execution;
  await expect(
    refract.run("error", () => client.responses.create(), {
      onComplete: (run) => {
        captured = run;
      },
    }),
  ).rejects.toBe(error);
  expect(captured.events[0].status).toBe("failed");
  expect(captured.status).toBe("failed");
});
it("samples out runs without skipping application logic or masking return values", async () => {
  let called = false;
  const result = await refract.run(
    "sampled",
    () => {
      refract.event({ type: "generation", name: "answer" });
      return 42;
    },
    {
      sampleRate: 0,
      onComplete: () => {
        called = true;
      },
    },
  );
  expect(result).toBe(42);
  expect(called).toBe(false);
});
it("marks partial streams and preserves stream failures", async () => {
  async function* stream() {
    yield { delta: "first" };
    throw new TypeError("stream lost");
  }
  const client = { responses: { create: () => stream() } };
  instrumentOpenAI(client);
  let captured!: Execution;
  await expect(
    refract.run(
      "broken stream",
      async () => {
        for await (const _chunk of client.responses.create()) {
          /* consume */
        }
      },
      {
        onComplete: (run) => {
          captured = run;
        },
      },
    ),
  ).rejects.toThrow("stream lost");
  expect(captured.events[0].status).toBe("failed");
  expect(captured.events[0].attributes?.stream_completed).toBe(false);
});

it("captures tool span results and isolates concurrent parent relationships", async () => {
  let captured!: Execution;
  await refract.run(
    "branches",
    () =>
      Promise.all(
        ["first", "second"].map((name) =>
          refract.span({ type: "tool.call", name }, async () => {
            await Promise.resolve();
            refract.event({ type: "retrieval", name: `${name}-child` });
            return { answer: name };
          }),
        ),
      ),
    {
      onComplete: (run) => {
        captured = run;
      },
    },
  );
  for (const name of ["first", "second"]) {
    const parent = captured.events.find((event) => event.name === name)!;
    const child = captured.events.find(
      (event) => event.name === `${name}-child`,
    )!;
    expect(child.parent_id).toBe(parent.id);
    expect(parent.output).toEqual({ answer: name });
  }
});

it("preserves provider promise helpers, stream handles and records a response once", async () => {
  const body = {
    model: "demo",
    output_text: "answer",
    usage: { input_tokens: 3, output_tokens: 2 },
  };
  const raw = new Response("not consumed");
  function create() {
    const promise = Promise.resolve(body);
    return Object.assign(promise, {
      withResponse: async () => ({
        data: body,
        response: raw,
        request_id: "req-1",
      }),
      asResponse: async () => raw,
    });
  }
  const client = { responses: { create } };
  instrumentOpenAI(client);
  let captured!: Execution;
  await refract.run(
    "helpers",
    async () => {
      const promise = client.responses.create();
      const envelope = await promise.withResponse();
      expect(envelope.data).toBe(body);
      expect(envelope.response).toBe(raw);
      expect(envelope.request_id).toBe("req-1");
      expect(await promise).toBe(body);
    },
    {
      onComplete: (value) => {
        captured = value;
      },
    },
  );
  expect(captured.events).toHaveLength(1);
  expect(captured.events[0].status).toBe("completed");
  await refract.run("raw response", async () => {
    const response = await client.responses.create().asResponse();
    expect(response).toBe(raw);
    expect(response.bodyUsed).toBe(false);
  });
});

it("records early stream cancellation once and closes the provider iterator", async () => {
  let closed = 0;
  async function* chunks() {
    try {
      yield { delta: "first" };
      yield { delta: "second" };
    } finally {
      closed++;
    }
  }
  const client = { responses: { create: () => chunks() } };
  instrumentOpenAI(client);
  let captured!: Execution;
  await refract.run(
    "cancel",
    async () => {
      for await (const chunk of client.responses.create()) {
        expect(chunk.delta).toBe("first");
        break;
      }
    },
    {
      onComplete: (value) => {
        captured = value;
      },
    },
  );
  expect(closed).toBe(1);
  expect(captured.events).toHaveLength(1);
  expect(captured.events[0].attributes?.stream_completed).toBe(false);
  expect(captured.events[0].output).toMatchObject({ text: "first" });
});

it("returns provider promises untouched outside capture and rejects token-shaped secrets", async () => {
  const promise = Promise.resolve({ output_text: "unchanged" });
  const client = { responses: { create: () => promise } };
  instrumentOpenAI(client);
  expect(client.responses.create()).toBe(promise);
  await refract.run(
    "sampled out",
    () => {
      expect(client.responses.create()).toBe(promise);
    },
    { sampleRate: 0 },
  );
  expect(
    redact({
      input_tokens: -1,
      output_tokens: 1.5,
      cache_write_tokens: 3,
      total_tokens: "secret",
    }),
  ).toEqual({
    input_tokens: "[REDACTED]",
    output_tokens: "[REDACTED]",
    cache_write_tokens: 3,
    total_tokens: "[REDACTED]",
  });
});
