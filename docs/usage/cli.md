# Offline CLI mode

`refract` is the Rust executable. Install from the checkout with `cargo install --path crates/refract-cli`
or prefix commands with `cargo run -p refract-cli --`. The Docker image also contains this executable.

```bash
refract doctor
refract pack tests/fixtures/simple-run/execution.json -o .examples/original.rfr
refract inspect .examples/original.rfr
refract validate .examples/original.rfr
refract replay .examples/original.rfr
refract fork .examples/original.rfr --from evt_2 -o .examples/branch.rfr
refract diff .examples/original.rfr .examples/branch.rfr
refract unpack .examples/original.rfr -o .examples/execution.json
refract serve
```

Create `.examples/` first. Output files use exclusive creation, preventing silent overwrites.
`inspect` prints a canonical JSON snapshot. `unpack` writes one JSON document, not extracted ZIP paths.
`pack` accepts canonical JSON or a legacy/text `.rfr`, and writes the new readable format.

`replay` returns recorded outputs; a `BLOCKED` event rejects playback. It does not rerun application code.
`fork` preserves the prefix **before** the selected event, creates a new run ID and records lineage.
`diff` compares ordered event semantics, ignoring generated IDs/timing. Exit 0 means no differences;
exit 1 can mean differences or a command error, so automation must validate output or use the Action runner.

## Experiment and evaluate

```bash
refract metrics baseline.rfr
refract diff baseline.rfr candidate.rfr --semantic --max-cost-increase-percent 10
refract eval tests/executions --report .examples/evaluation-report.json
refract rerun baseline.rfr --from evt_2 --executor python3 \
  --executor-arg examples/rerun/executor.py --model candidate --allow-live -o branch.rfr
```

See [metrics](metrics.md), [executable rerun](rerun.md), and [semantic evaluation](evaluation.md)
for schemas, trust boundaries, exit codes and runnable examples.
