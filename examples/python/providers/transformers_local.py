"""Record local Transformers inference with existing weights; downloads are disabled."""

import argparse
from pathlib import Path

import refract


def main():
    from transformers import AutoModelForCausalLM, AutoTokenizer, pipeline

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model-path", required=True)
    parser.add_argument("--output", type=Path, default=Path(".examples/transformers.rfr"))
    args = parser.parse_args()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    tokenizer = AutoTokenizer.from_pretrained(args.model_path, local_files_only=True)
    weights = AutoModelForCausalLM.from_pretrained(args.model_path, local_files_only=True)
    generate = pipeline("text-generation", model=weights, tokenizer=tokenizer)

    class LocalModel:
        def generate(self, prompt):
            return generate(prompt, max_new_tokens=32)

    model = LocalModel()
    handle = refract.instrument_custom(
        model,
        "generate",
        provider="transformers",
        model=Path(args.model_path).name,
        request=lambda call_args, kwargs: {"input": call_args[0]},
    )
    try:
        with refract.run("local-transformers", path=args.output, fail_open=False):
            print(model.generate("Hello, my name is"))
    finally:
        handle.uninstrument()


if __name__ == "__main__":
    main()
