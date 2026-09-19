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
      - uses: ./
        with:
          baseline: examples/artifacts/demo.rfr
          actual: .examples/actual.rfr
```

That relative Action path applies inside this repository. In another repository, reference a tagged
release instead:

```yaml
- uses: khaleddeissa/llm-refract@v0.1.1
```

The action metadata lives at the repo root. This repository also contains CI workflows, which the
GitHub Marketplace listing requirements disallow — so it is not listed there, but referencing it by
tag works identically either way.

The composite Action builds the Rust CLI, validates both inputs and writes `refract-report.json`.
Differences or invalid artifacts fail the step. The report records event-level differences; thresholds,
semantic model graders and remote datasets are not implemented.

```bash
python3 packages/github-action/compare.py examples/artifacts/demo.rfr .examples/actual.rfr \
  --cli target/debug/refract --report .examples/regression-report.json
```

Review baseline updates as code changes. Upload reports/recordings with your CI artifact step only
under your own data-retention policy. Repository CI runs on pushes/PRs/manual dispatch, with no scheduled
jobs. Dependabot update checks are monthly. Tagged releases publish container images, npm and PyPI
packages via `.github/workflows/publish.yml`.
