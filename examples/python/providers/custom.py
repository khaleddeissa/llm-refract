"""Offline custom callable; replace generate with your own inference function."""

import argparse
from pathlib import Path

import refract


class LocalModel:
    def generate(self, prompt: str) -> dict:
        # Deterministic demonstration, not an LLM or a fabricated provider response.
        return {"text": prompt.upper()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=Path(".examples/custom.rfr"))
    args = parser.parse_args()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    model = LocalModel()
    handle = refract.instrument_custom(
        model,
        "generate",
        provider="custom",
        model="offline-demo",
        request=lambda call_args, kwargs: {"input": call_args[0]},
    )
    try:
        with refract.run("custom-provider", path=args.output, fail_open=False):
            print(model.generate("hello local inference"))
    finally:
        handle.uninstrument()


if __name__ == "__main__":
    main()
