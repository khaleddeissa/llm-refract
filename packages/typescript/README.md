<div align="center">

<img src="https://raw.githubusercontent.com/khaleddeissa/llm-refract/main/assets/llm-refract-logo.svg" width="96" height="96" alt="Refract green diamond" />

# @llm-refract/sdk

**Node.js SDK for Refract — a portable execution recording, replay and diff engine for AI systems.**

[![npm](https://img.shields.io/npm/v/%40llm-refract%2Fsdk?style=for-the-badge&label=npm&color=CB3837)](https://www.npmjs.com/package/@llm-refract/sdk)
[![Downloads](https://img.shields.io/npm/dm/%40llm-refract%2Fsdk?style=for-the-badge&color=2EA44F)](https://www.npmjs.com/package/@llm-refract/sdk)
[![Node](https://img.shields.io/badge/Node-%3E%3D22.12-339933?style=for-the-badge&logo=node.js&logoColor=white)](https://nodejs.org/)
[![TypeScript](https://img.shields.io/badge/TypeScript-Ready-3178C6?style=for-the-badge&logo=typescript&logoColor=white)](#)
[![License](https://img.shields.io/badge/License-Apache--2.0-0D75B8?style=for-the-badge)](https://github.com/khaleddeissa/llm-refract/blob/main/LICENSE)

[Docs](https://github.com/khaleddeissa/llm-refract/tree/main/docs) · [Examples](https://github.com/khaleddeissa/llm-refract/tree/main/examples/typescript) · [Source](https://github.com/khaleddeissa/llm-refract/tree/main/packages/typescript) · [Issues](https://github.com/khaleddeissa/llm-refract/issues)

</div>

---

Targets server-side Node.js apps and agents. Uses `AsyncLocalStorage`, filesystem and crypto APIs —
not a browser SDK. Record model calls, tools, retrieval, decisions, state changes, checkpoints and
failures as a portable `.rfr` artifact, or submit them live to a Refract endpoint.

## Install

```bash
npm install @llm-refract/sdk
```

## Usage

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

Omit `endpoint` for offline recording, or `path` for API-only submission. `onComplete(execution)` gives
a cloned snapshot after capture/export. Capture and export fail open by default; set `onError` to
observe failures or `failOpen: false` for strict recording. Application errors always propagate.

Wrap configured OpenAI, Anthropic, Azure, Gemini, Vertex or Bedrock clients for provider capture,
or use `instrumentCustom` for open-source and application-specific backends. OpenAI-compatible local
endpoints use the same wrapper. Async streams retain chunk identity and record partial consumption,
errors, token counts and first-text latency. Credentials and model selection stay in your application.

`BatchExporter` adds bounded background ingestion, retries and optional local disk recovery.
`refract.span` records nested tools and framework calls; `toOtlp`/`fromOtlp` bridge existing traces.
`exportLangfuse` explicitly exports completed recordings to a configured Langfuse instance.

`pack`/`unpack` read and write the text artifact profile and verify checksums on read. The Rust CLI
reads legacy ZIP recordings; TypeScript types describe canonical events, with authoritative ingestion
validation in the Rust engine.

Try `node examples/typescript/basic/record.mjs` or `node examples/typescript/concurrent/record.mjs`
after building the SDK. Outputs go to `.examples/`.

See the [TypeScript usage guide](https://github.com/khaleddeissa/llm-refract/blob/main/docs/usage/typescript.md)
for more.

## Links

- [Documentation](https://github.com/khaleddeissa/llm-refract/tree/main/docs)
- [Python SDK](https://pypi.org/project/llm-refract/)
- [Rust CLI and engine](https://github.com/khaleddeissa/llm-refract/tree/main/crates)
- [Issues](https://github.com/khaleddeissa/llm-refract/issues)

## License

Apache-2.0 — see [LICENSE](https://github.com/khaleddeissa/llm-refract/blob/main/LICENSE).
