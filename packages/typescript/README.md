# @refract-ai/sdk

Node.js SDK for Refract — a portable execution recording, replay and diff engine for AI systems.
Targets server-side Node.js apps and agents (uses `AsyncLocalStorage`, filesystem and crypto APIs;
not a browser SDK).

## Install

```bash
npm install @refract-ai/sdk
```

## Usage

```typescript
import { refract, unpack } from "@refract-ai/sdk";
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

Omit `endpoint` for offline recording, or `path` for API-only submission. `pack`/`unpack` read and
write the text artifact profile and verify checksums on read.

## Links

- [Documentation](https://github.com/khaleddeissa/llm-refract/tree/main/docs)
- [Source](https://github.com/khaleddeissa/llm-refract/tree/main/packages/typescript)
- [Issues](https://github.com/khaleddeissa/llm-refract/issues)

License: Apache-2.0
