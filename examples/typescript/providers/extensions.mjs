// Offline fixtures: no model download, provider credentials or network requests.
import { EventEmitter } from "node:events";
import { mkdir } from "node:fs/promises";
import {
  refract,
  instrumentLibrary,
  instrumentRealtime,
} from "../../../packages/typescript/dist/index.js";

const local = {
  async chat() {
    return {
      message: { content: "Synthetic local answer" },
      prompt_eval_count: 8,
      eval_count: 4,
    };
  },
};
const restore = instrumentLibrary(local, "ollama");
await mkdir(".examples", { recursive: true });
await refract.run(
  "local-and-realtime-fixtures",
  async () => {
    await local.chat({
      model: "fixture-model",
      messages: [{ role: "user", content: "hello" }],
    });
    const connection = new EventEmitter();
    const stop = instrumentRealtime(connection, {
      provider: "custom",
      model: "fixture-voice",
    });
    connection.emit("event", {
      type: "response.created",
      response: { id: "response-fixture" },
    });
    connection.emit("event", {
      type: "response.output_text.delta",
      response_id: "response-fixture",
      delta: "Synthetic voice transcript",
    });
    connection.emit("event", {
      type: "response.done",
      response: {
        id: "response-fixture",
        status: "completed",
        usage: { input_tokens: 6, output_tokens: 3 },
      },
    });
    stop();
  },
  { path: ".examples/provider-extensions.rfr" },
);
restore();
console.log(
  "Wrote .examples/provider-extensions.rfr; counts are explicit synthetic fixtures.",
);
