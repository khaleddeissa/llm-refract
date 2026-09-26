# Executable branch example

Run from the repository root after building the CLI:

```bash
mkdir -p .examples
refract rerun examples/artifacts/demo.rfr --from evt_2 \
  --executor python3 --executor-arg examples/rerun/executor.py \
  --model local-candidate --allow-live -o .examples/rerun.rfr
refract diff examples/artifacts/demo.rfr .examples/rerun.rfr --semantic
```

The explicit executable reads one JSON request containing `event` and `context`, obtains the return
window from the retained retrieval output, and computes a fresh answer. It does not call a model.
A real executor can call a provider using the supplied model and application credentials. Existing
output files are not overwritten. See [executable replay](../../docs/usage/rerun.md).
