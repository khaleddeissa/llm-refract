# Refract execution regression Action

Compares fresh captured behavior against a reviewed baseline using the Rust engine.
The action metadata now lives at the repo root (`/action.yml`) so it can be referenced as
`khaleddeissa/llm-refract@<tag>`. It is still not listed on the GitHub Marketplace, since this
repository also contains CI workflows, which Marketplace listing disallows.

See [usage](../../docs/usage/ci.md). Run locally with
`python3 packages/github-action/compare.py BASELINE ACTUAL --cli target/debug/refract`.
