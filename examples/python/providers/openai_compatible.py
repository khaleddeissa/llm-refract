"""Call an existing Ollama, vLLM, or other OpenAI-compatible endpoint explicitly."""

import argparse
import os
from pathlib import Path

import refract


def main():
    from openai import OpenAI

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-url", required=True)
    parser.add_argument("--provider", required=True)
    parser.add_argument("--model", required=True)
    parser.add_argument("--output", type=Path, default=Path(".examples/local-provider.rfr"))
    args = parser.parse_args()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with OpenAI(base_url=args.base_url, api_key=os.getenv("LLM_API_KEY", "local")) as client:
        handle = refract.instrument_custom(client.chat.completions, "create", provider=args.provider)
        try:
            with refract.run("local-provider", path=args.output, fail_open=False):
                response = client.chat.completions.create(
                    model=args.model, messages=[{"role": "user", "content": "Say hello briefly."}]
                )
                print(response.choices[0].message.content)
        finally:
            handle.uninstrument()


if __name__ == "__main__":
    main()
