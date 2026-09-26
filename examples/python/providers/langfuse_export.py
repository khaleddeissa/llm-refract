"""Convert an artifact offline; send only with --send and Langfuse project credentials."""

import argparse
import json
import os
from pathlib import Path

from refract.artifact import unpack
from refract.integrations.langfuse import export_langfuse, to_langfuse


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("artifact", type=Path)
    parser.add_argument("--send", action="store_true")
    args = parser.parse_args()
    run = unpack(args.artifact.read_bytes())
    if args.send:
        export_langfuse(
            run,
            os.environ["LANGFUSE_BASE_URL"],
            public_key=os.environ["LANGFUSE_PUBLIC_KEY"],
            secret_key=os.environ["LANGFUSE_SECRET_KEY"],
        )
    else:
        print(json.dumps(to_langfuse(run), indent=2))


if __name__ == "__main__":
    main()
