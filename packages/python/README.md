# Refract Python SDK

Instrument synchronous and asynchronous AI applications with provider-neutral events.
Export portable `.rfr` files or submit snapshots to the Refract REST service.

```python
import refract

with refract.run("agent", path="run.rfr"):
    refract.event(type="generation", name="answer", output={"text": "Hello"})
```

See the [usage guide](https://github.com/khaleddeissa/llm-refract/blob/main/docs/usage/python.md).
