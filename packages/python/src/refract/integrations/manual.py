"""Provider-neutral adapter for any synchronous callable that returns JSON data."""

import time
from collections.abc import Callable
from typing import Any

import refract


def generation(call: Callable[[], Any], *, provider: str, model: str, prompt: Any) -> Any:
    start = time.perf_counter()
    try:
        result = call()
    except Exception as error:
        refract.event(
            type="generation",
            name=f"{provider}/{model}",
            input=prompt,
            output={"exception_type": type(error).__name__},
            status="failed",
            duration_ms=(time.perf_counter() - start) * 1000,
            attributes={"provider": provider, "model": model},
        )
        raise
    refract.event(
        type="generation",
        name=f"{provider}/{model}",
        input=prompt,
        output=result,
        duration_ms=(time.perf_counter() - start) * 1000,
        attributes={"provider": provider, "model": model},
    )
    return result
