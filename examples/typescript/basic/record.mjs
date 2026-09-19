import { mkdir } from "node:fs/promises";
await mkdir(".examples", { recursive: true });
import { refract } from "../../../packages/typescript/dist/index.js";
await refract.run(
  "customer-support",
  () => {
    refract.event({
      type: "generation",
      name: "Draft answer",
      output: { text: "Returns within 30 days." },
      attributes: { provider: "demo" },
    });
  },
  { path: ".examples/typescript-example.rfr" },
);
