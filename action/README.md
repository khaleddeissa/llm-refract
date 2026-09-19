# Execution regression action

Compare a newly generated execution against a checked-in baseline using the Rust validator and diff engine.
This is a working repository action; it has not been published to GitHub Marketplace.

```yaml
- uses: actions/checkout@v4
# Run your application/instrumented tests to produce actual.rfr first.
- uses: ./action
  with:
    baseline: tests/fixtures/simple-run/python.rfr
    actual: actual.rfr
```

Local equivalent:

```bash
python3 action/compare.py baseline.rfr actual.rfr --cli target/debug/refract
```

The check writes `refract-report.json` and fails on semantic changes or invalid input.
It compares fresh captured behavior; replaying a baseline against itself is not a regression test.
Remote datasets, thresholds and live execution are not part of this action yet.
