import { expect, test } from "vitest";
import {
  instrumentCustom,
  instrumentOpenAI,
  refract,
  type Execution,
} from "../src/index.js";

test.each([false, true])(
  "nested gateway/provider capture records one generation (stream=%s)",
  async (streaming) => {
    const client = {
      responses: {
        create: async () =>
          streaming
            ? (async function* () {
                yield {
                  delta: "answer",
                  usage: { input_tokens: 3, output_tokens: 2 },
                };
              })()
            : {
                output_text: "answer",
                usage: { input_tokens: 3, output_tokens: 2 },
              },
      },
    };
    const restoreProvider = instrumentOpenAI(client);
    const gateway = { create: async () => client.responses.create() };
    const restoreGateway = instrumentCustom(gateway, {
      provider: "gateway",
      methods: [["create"]],
    });
    let captured!: Execution;
    try {
      await refract.run(
        "nested",
        async () => {
          const result = await gateway.create();
          if (Symbol.asyncIterator in result)
            for await (const _chunk of result) {
              /* consume */
            }
        },
        {
          onComplete: (run) => {
            captured = run;
          },
        },
      );
      expect(captured.events).toHaveLength(1);
      expect(captured.events[0].attributes).toMatchObject({
        provider: "gateway",
        total_tokens: 5,
      });
    } finally {
      restoreGateway();
      restoreProvider();
    }
  },
);

test("independent concurrent calls and calls after a failure remain separate", async () => {
  const client = {
    responses: {
      create: async (fail = false) => {
        await Promise.resolve();
        if (fail) throw new Error("fixture");
        return { output_text: "ok" };
      },
    },
  };
  const restoreProvider = instrumentOpenAI(client);
  const gateway = {
    create: async (fail = false) => client.responses.create(fail),
  };
  const restoreGateway = instrumentCustom(gateway, {
    provider: "gateway",
    methods: [["create"]],
  });
  let captured!: Execution;
  try {
    await refract.run(
      "concurrent",
      async () => {
        await Promise.allSettled([gateway.create(true), gateway.create()]);
        await client.responses.create();
      },
      {
        onComplete: (run) => {
          captured = run;
        },
      },
    );
    expect(captured.events).toHaveLength(3);
    expect(captured.events.filter((e) => e.status === "failed")).toHaveLength(
      1,
    );
  } finally {
    restoreGateway();
    restoreProvider();
  }
});
