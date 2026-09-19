# Python application mode

The Python SDK records manual events, isolates concurrent runs with context variables, redacts
sensitive keys and writes artifacts or posts completed snapshots to the Rust API. It has no model-provider
dependency. See [installation](../development.md) to consume the package from source.

```python
import refract

with refract.run("checkout", path="checkout.rfr") as run:
    parent = refract.event(type="tool.call", name="lookup", output={"price": 120})
    refract.event(
        type="state.change",
        name="discount",
        parent_id=parent,
        input={"total": 120},
        output={"total": 90},
    )
snapshot = run.snapshot()
```

Add `endpoint="http://localhost:8000"` to upload at context exit. Both file export and network submission
are synchronous. Recording errors propagate when the application succeeded; if application code already
failed, its original exception is preserved and the recording error is attached as a note. Budget for
this latency/failure behavior when instrumenting production code.

Use a normal `with refract.run(...)` inside an async function. Context variables isolate concurrent
coroutines. `@refract.trace` supports sync/async entrypoints, but currently creates an in-memory run;
use an explicit run with `path`/`endpoint` for persistent evidence. Decorators do not capture arguments
or exception messages automatically.

For a synchronous JSON-returning provider call, `refract.integrations.manual.generation` records
provider/model, input, output and duration. It must run inside an active run context. Provider response
objects must be converted to JSON-compatible values by the application.

```python
from refract.integrations.manual import generation

with refract.run("custom-model", path="custom.rfr"):
    answer = generation(lambda: {"text": "Hello"}, provider="custom", model="demo", prompt="Hi")
```

[Examples](../../examples/README.md) cover RAG documents/citations, failures, state checkpoints and
explicit parent-child relationships. [Artifact APIs](artifacts.md) cover `pack` and `unpack`.
