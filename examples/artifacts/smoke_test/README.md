# Recorded smoke-test scenarios

These are the nine saved outputs moved from `.examples/`. They demonstrate Python and Node recording,
redaction, failure capture, state/checkpoint policies, concurrent run isolation and regression comparison.
They are synthetic examples, not real customer traffic or responses from a live model. No model API,
retrieval service or payment system was called. The SDKs recorded the values supplied by the example code.

| File                                             | Source and scenario                                                                                | Where the values come from                                                                                                                                                                     |
| ------------------------------------------------ | -------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [example.rfr](example.rfr)                       | [Python basic](../../python/basic/record.py): retrieval followed by generation                     | Explicit 30-day policy and a hard-coded answer; the answer event references the retrieval event.                                                                                               |
| [actual.rfr](actual.rfr)                         | [Python regression](../../python/regression/record.py): fresh execution compared with the baseline | Local `days = 30` and an f-string produce the answer matching [demo.rfr](../demo.rfr). It is not a copy of the baseline.                                                                       |
| [rag.rfr](rag.rfr)                               | [Python RAG](../../python/rag/record.py): retrieval and cited answer                               | A supplied document named `returns-v1`, its 30-day policy, and an explicit citation. No vector database or LLM is involved.                                                                    |
| [failure.rfr](failure.rfr)                       | [Python failure](../../python/failure/record.py): failed customer lookup                           | The lookup records `found: false`; an intentional `LookupError` marks the run failed. The SDK records failure information without copying the exception message. The demo API key is redacted. |
| [state.rfr](state.rfr)                           | [Python state](../../python/state/record.py): state change and checkpoint                          | Explicit totals of 120 and 90 represent a discount; payment remains `not_started`. The checkpoint uses `BLOCKED`, so recorded playback deliberately refuses that step.                         |
| [typescript-example.rfr](typescript-example.rfr) | [Node basic](../../typescript/basic/record.mjs): one generation event                              | A hard-coded 30-day answer and `provider: demo`.                                                                                                                                               |
| [alpha.rfr](alpha.rfr), [beta.rfr](beta.rfr)     | [Node concurrent](../../typescript/concurrent/record.mjs): two async runs                          | `Promise.all` runs the `alpha` and `beta` callbacks concurrently. Each lookup records its own team name; async context isolates the runs and the demo API key becomes `[REDACTED]`.            |
| [regression-report.json](regression-report.json) | [Action comparison runner](../../../packages/github-action/compare.py)                             | The Rust CLI validates baseline and actual artifacts, then compares ordered event semantics. No differences yields `passed: true` and an empty `differences` array.                            |

## How recording metadata was produced

SDKs generate run/event identifiers and capture timestamps from the clock when examples execute.
Parent IDs link events within each run. Run status reflects successful completion or an exception;
event durations of zero mean the example did not supply measured durations. These timestamps and IDs
are saved historical values, not guarantees about future executions. Re-running changes metadata and
checksums even when the application outputs stay the same.

Each `.rfr` is UTF-8: one JSON header followed by formatted execution JSON. Its SHA-256 covers the exact
payload bytes after the first newline. Validate with the CLI; editing payload text without repacking
invalidates that checksum. See the [artifact format guide](../../../docs/usage/artifacts.md).

## Reproduce from the repository root

After the [development setup](../../../docs/development.md):

```bash
uv run python examples/python/basic/record.py
uv run python examples/python/failure/record.py
uv run python examples/python/rag/record.py
uv run python examples/python/state/record.py
uv run python examples/python/regression/record.py
npm run build -w @refract-ai/sdk
node examples/typescript/basic/record.mjs
node examples/typescript/concurrent/record.mjs
cargo build -p refract-cli
uv run python packages/github-action/compare.py \
  examples/artifacts/demo.rfr .examples/actual.rfr \
  --cli target/debug/refract --report .examples/regression-report.json
```

Fresh outputs still go to ignored `.examples/`, so these commands do not overwrite the checked-in
snapshots. The saved report is evidence of the baseline comparison at recording time, not a claim that
future code changes pass. Generate a fresh execution before running regression checks.

To validate one of the committed snapshots:

```bash
target/debug/refract validate examples/artifacts/smoke_test/actual.rfr
target/debug/refract inspect examples/artifacts/smoke_test/failure.rfr
```
