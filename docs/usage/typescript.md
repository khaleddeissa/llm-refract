# TypeScript / npm application mode

`@refract-ai/sdk` targets Node.js, including server-side web applications and agents. It uses Node
`AsyncLocalStorage`, filesystem and crypto APIs; it is not a browser SDK. Install with
`npm install @llm-refract/sdk`; see [installation](../development.md) to build from source instead.

```typescript
import { refract, unpack } from "@llm-refract/sdk";
import { readFile } from "node:fs/promises";

const answer = await refract.run(
  "agent",
  async () => {
    const output = { text: "Hello" };
    refract.event({
      type: "generation",
      name: "answer",
      output,
      attributes: { provider: "local", model: "demo" },
    });
    return output;
  },
  { path: "agent.rfr", endpoint: "http://localhost:8000" },
);
const recorded = unpack(await readFile("agent.rfr"));
```

Omit `endpoint` for offline recording or `path` for API-only submission. `onComplete(execution)` gives
a cloned snapshot after capture/export. Recording/export errors reject an otherwise successful call;
application errors take priority. HTTP calls have a timeout. Background tasks must finish before their
run callback returns; late events are rejected.

`pack`/`unpack` write/read the text artifact profile and verify checksums on read. The Rust CLI reads
legacy ZIP recordings. TypeScript types describe canonical events; authoritative ingestion validation
still occurs in the Rust engine.

Try `node examples/typescript/basic/record.mjs` or `node examples/typescript/concurrent/record.mjs`
after building the SDK. Outputs go to `.examples/`. Tests cover concurrent isolation, failure capture,
redaction, readable round trips and checksum tampering.
