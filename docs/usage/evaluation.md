# Semantic comparison and evaluation datasets

`refract diff old.rfr new.rfr` keeps the existing strict ordered comparison. Add `--semantic` to grade
outputs while retaining checks on event type/name, status, input, parent position and replay policy.
Model/provider labels and measurement attributes are excluded from the behavioral comparison, allowing
model experiments to compare the resulting behavior and budgets separately.

```bash
refract diff baseline.rfr candidate.rfr --semantic --threshold 0.85
refract diff baseline.rfr candidate.rfr --semantic \
  --max-cost-increase-percent 10 --max-latency-increase-percent 20 \
  --max-token-increase-percent 5
```

Exit 0 means the comparison passed, 1 means a regression/budget failure, and other errors mean the
comparison could not run. A requested budget fails if its measurements are incomplete. Unknown cost
is not treated as free. Metrics report both captured totals and coverage; supply provider usage and
explicit prices to evaluate costs.

## Offline and custom graders

The built-in `offline-token-overlap-v1` grader uses normalized token overlap, a small synonym vocabulary,
and number/negation checks. It catches a 30-to-14 day change and recognizes a narrow set of paraphrases.
It is an explainable heuristic, not a language model or a guarantee of semantic equivalence. Domain
facts, subtle contradictions and tone require a suitable custom grader or review.

Rust applications implement `refract_diff::Grader`. CLI users can provide an explicit executable:

```bash
refract diff baseline.rfr candidate.rfr --semantic \
  --grader-command python3 --grader-arg my_grader.py
```

The executable receives one JSON object on stdin with `left`, `right`, and `threshold`. It returns:

```json
{
  "score": 0.95,
  "equivalent": true,
  "reason": "Same return window",
  "grader": "my-domain-grader-v1"
}
```

Use stderr for logs. Output is bounded to 1 MiB and each invocation has a 60-second timeout. The command
is executed directly, without a shell. It must be explicitly selected on the CLI; artifacts cannot
supply a command. A custom grader may call your preferred model or embedding service and is responsible
for its credentials, privacy, cost and deterministic configuration. Invalid scores and failures fail
the comparison. The hosted REST/MCP paths use the offline grader and never execute command plugins.

## Dataset manifests

Create a directory with `dataset.json` plus baseline and freshly generated candidate recordings:

```json
{
  "version": 1,
  "options": { "similarity_threshold": 0.85 },
  "cases": [
    {
      "name": "refund",
      "baseline": "baseline/refund.rfr",
      "candidate": "candidate/refund.rfr"
    },
    {
      "name": "tool-error",
      "baseline": "baseline/tool-error.rfr",
      "candidate": "candidate/tool-error.rfr",
      "options": { "similarity_threshold": 1.0, "max_cost_increase_percent": 5 }
    }
  ]
}
```

Case options replace dataset options for that case. Paths stay inside the dataset directory; duplicate
case names, an empty dataset, and unsupported versions are rejected. Missing/corrupt recordings become
failed cases rather than disappearing from the result. Generate candidate recordings from the code
under test; evaluation itself does not run your application.

```bash
refract eval tests/executions --report .examples/evaluation-report.json
# The same explicit custom grader interface is supported:
refract eval tests/executions --grader-command python3 --grader-arg my_grader.py
```

The JSON report contains per-case explanations, budgets, baseline/candidate measurements and totals.
“Improved” means a passing case with lower measured cost or wall time; it does not imply higher answer
quality. Existing report files are not overwritten. See the [runnable two-case example](../../examples/evaluation/README.md).

## CI and remote runs

The root GitHub Action accepts `comparison-options`, a path to a JSON file containing the same
similarity and metric budget options. It keeps strict comparison when that input is omitted. For a
whole dataset, generate candidates and run `refract eval` in a CI step, then upload the JSON report as
an artifact even if evaluation fails. See [CI usage](ci.md).

The API also compares stored runs through `POST /v1/eval` using
`{"pairs":[{"name":"refund","left":"baseline-id","right":"candidate-id"}],"options":{}}`.
MCP exposes this as `evaluate_runs`. Every referenced run must be visible to the authenticated scope.
