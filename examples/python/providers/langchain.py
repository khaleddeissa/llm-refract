"""Offline LangChain callback recording; no model provider is called."""

import argparse
from pathlib import Path

import refract
from refract.integrations.langchain import langchain_handler


def main():
    from langchain_core.runnables import RunnableLambda

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=Path(".examples/langchain.rfr"))
    args = parser.parse_args()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    chain = RunnableLambda(lambda query: {"answer": query.upper()})
    with refract.run("langchain-example", path=args.output, fail_open=False):
        print(chain.invoke("hello", config={"callbacks": [langchain_handler()]}))


if __name__ == "__main__":
    main()
