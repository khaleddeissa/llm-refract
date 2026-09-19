# Regression and GitHub CI mode

A regression check compares **new behavior** from the application under test with a reviewed baseline.
Replaying a saved artifact against itself only verifies playback, not a code change.

```yaml
name: Execution regression
on: [push, pull_request]
jobs:
  compare:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      # Run your instrumented application/tests here to produce .examples/actual.rfr.
      - uses: ./packages/github-action
        with:
          baseline: examples/artifacts/demo.rfr
          actual: .examples/actual.rfr
```

That relative Action path applies inside this repository. In another repository, check out Refract into
a separate directory and use `./path-to-refract/packages/github-action`, or reference a reviewed commit
as `khaleddeissa/llm-refract/packages/github-action@<commit>`. No Marketplace release is claimed.

The composite Action builds the Rust CLI, validates both inputs and writes `refract-report.json`.
Differences or invalid artifacts fail the step. The report records event-level differences; thresholds,
semantic model graders and remote datasets are not implemented.

```bash
python3 packages/github-action/compare.py examples/artifacts/demo.rfr .examples/actual.rfr \
  --cli target/debug/refract --report .examples/regression-report.json
```

Review baseline updates as code changes. Upload reports/recordings with your CI artifact step only
under your own data-retention policy. Repository CI runs on pushes/PRs/manual dispatch, with no scheduled
jobs. Dependabot update checks are monthly. Release workflows build candidates without publishing.
