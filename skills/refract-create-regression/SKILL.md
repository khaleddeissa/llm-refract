---
name: refract-create-regression
description: Create a Refract baseline comparison for newly recorded application behavior.
---

# refract-create-regression

Run from the repository root. Use `cargo run -p refract-cli --` in place of `refract` when the CLI is not installed.

Choose a reviewed baseline and rerun the instrumented application to produce a separate actual artifact. Run `python3 packages/github-action/compare.py BASELINE ACTUAL --cli target/debug/refract`; inspect refract-report.json. Use ./packages/github-action in this repository for CI. Never replace the baseline automatically just to make a test pass, and never call playback against the same baseline a regression test.
