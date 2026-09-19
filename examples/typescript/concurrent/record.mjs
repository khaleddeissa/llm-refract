import { refract } from "../../../packages/typescript/dist/index.js";
await Promise.all(
  ["alpha", "beta"].map((name) =>
    refract.run(
      name,
      async () => {
        await Promise.resolve();
        refract.event({
          type: "tool.call",
          name: "lookup",
          input: { api_key: "demo-secret" },
          output: { team: name },
        });
      },
      { path: `${name}.rfr` },
    ),
  ),
);
