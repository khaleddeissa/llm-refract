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

## Configured model grading in the service

A [generation profile](rerun.md#server-inspector-sdk-and-mcp-model-reruns) with `grading_rubric`
can grade stored runs. The rubric is operator-owned; the compared outputs are delimited JSON data.
Refract checks event structure and policies first, grades only compatible changed outputs, and
applies the same token/cost/latency budgets afterward. Identical outputs need no provider call.

```bash
refract compare-runs baseline-id candidate-id --grader refund-domain --allow-live --threshold 0.9
```

API `/v1/diff` and `/v1/eval` accept `grader` and `allow_live` alongside their existing options.
In the Inspector choose **Semantic grader** and enable **Authorize model grading calls**.
MCP `grade_runs` is available only with `REFRACT_MCP_ALLOW_LIVE=1`; Python and Node service clients
provide `compare(..., grader=..., allow_live=True)` / `compare(..., {grader, allow_live:true})`.

A model must return only JSON `{score,equivalent,reason}`, with a finite score in 0..1 and a nonempty
reason. Malformed/unavailable grading produces `grader_error` and a failing report. No heuristic
fallback silently turns model failures into passes. Each comparison permits 32 distinct changed-output
pairs and a 120-second grading deadline. Budget failures remain failures even if the model approves the
meaning. Model judgments depend on the configured rubric and model; review them as evidence, not proof.
