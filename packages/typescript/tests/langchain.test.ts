import { expect, test } from "vitest";
import { RunnableLambda } from "@langchain/core/runnables";
import { FakeListChatModel } from "@langchain/core/utils/testing";
import { AIMessage } from "@langchain/core/messages";
import {
  refract,
  langchainHandler,
  instrumentLangChainModel,
  instrumentOpenAI,
  type Execution,
} from "../src/index.js";

test("installed LangChain runnable produces ordered causal callbacks", async () => {
  let recording!: Execution;
  const handler = langchainHandler();
  const chain = RunnableLambda.from((input: string) =>
    input.toUpperCase(),
  ).pipe(RunnableLambda.from((input: string) => ({ answer: input })));
  await refract.run(
    "langchain",
    async () => {
      expect(await chain.invoke("hello", { callbacks: [handler] })).toEqual({
        answer: "HELLO",
      });
    },
    {
      onComplete: (run) => {
        recording = run;
      },
    },
  );
  expect(recording.events).toHaveLength(3);
  expect(recording.events.every((e) => e.status === "completed")).toBe(true);
  expect(recording.events[1].parent_id).toBe(recording.events[0].id);
  expect(recording.events[2].parent_id).toBe(recording.events[0].id);
  expect(handler.errors).toEqual([]);
});

test.each([false, true])(
  "model wrapper and callbacks retain one streamed/nonstreamed generation (%s)",
  async (streaming) => {
    const model = new FakeListChatModel({ responses: ["local answer"] });
    const restore = instrumentLangChainModel(model);
    const handler = langchainHandler();
    let recording!: Execution;
    try {
      await refract.run(
        "langchain model",
        async () => {
          if (streaming) {
            const response = await model.stream("hello", {
              callbacks: [handler],
            });
            let text = "";
            for await (const chunk of response) text += chunk.content;
            expect(text).toBe("local answer");
          } else
            expect(
              (await model.invoke("hello", { callbacks: [handler] })).content,
            ).toBe("local answer");
        },
        {
          onComplete: (run) => {
            recording = run;
          },
        },
      );
      expect(recording.events).toHaveLength(1);
      expect(recording.events[0].status).toBe("completed");
      expect(handler.errors).toEqual([]);
    } finally {
      restore();
    }
  },
);

test("LangChain model, callbacks and provider wrapper share one generation with usage", async () => {
  const client = {
    responses: {
      create: async () => ({
        output_text: "hello",
        usage: { input_tokens: 3, output_tokens: 2 },
      }),
    },
  };
  class Model extends FakeListChatModel {
    override async _generate() {
      const result = await client.responses.create();
      return {
        generations: [
          {
            text: result.output_text,
            message: new AIMessage({
              content: result.output_text,
              usage_metadata: { ...result.usage, total_tokens: 5 },
            }),
          },
        ],
      };
    }
  }
  const model = new Model({ responses: ["unused"] });
  const restoreProvider = instrumentOpenAI(client);
  const restoreModel = instrumentLangChainModel(model);
  const handler = langchainHandler();
  let recording!: Execution;
  try {
    await refract.run(
      "combined",
      async () => {
        await model.invoke("hello", { callbacks: [handler] });
      },
      {
        onComplete: (run) => {
          recording = run;
        },
      },
    );
    expect(recording.events).toHaveLength(1);
    expect(recording.events[0].attributes?.total_tokens).toBe(5);
    expect(handler.errors).toEqual([]);
  } finally {
    restoreModel();
    restoreProvider();
  }
});
