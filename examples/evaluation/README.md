# Dataset evaluation

From the repository root, run `uv run python examples/evaluation/generate.py`, then
`cargo run -p refract-cli -- eval .examples/evaluation`.

The script executes a local policy function three times. The dataset compares two candidates against
the 30-day baseline: one preserves the policy and one changes it to 14 days. Evaluation intentionally
exits with status 1 and reports one regression. No language model is called and no price is invented.
Change the candidate implementation in the generator to experiment with actual application behavior.

See [evaluation and graders](../../docs/usage/evaluation.md) for budgets, custom model graders and CI.
