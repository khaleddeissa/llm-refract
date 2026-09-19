"""Compare a baseline and freshly recorded artifact using the shared Rust engine."""

import argparse
import json
import subprocess
from pathlib import Path


def compare(baseline: Path, actual: Path, cli: str) -> dict:
    # Validate separately so an invalid artifact cannot be confused with a semantic difference.
    for path in (baseline, actual):
        subprocess.run([cli, "validate", str(path.resolve())], check=True, capture_output=True)
    result = subprocess.run(
        [cli, "diff", str(baseline.resolve()), str(actual.resolve())],
        check=False,
        text=True,
        capture_output=True,
    )
    if result.returncode not in (0, 1):
        raise RuntimeError(result.stderr)
    differences = json.loads(result.stdout)
    if not isinstance(differences, list):
        raise TypeError("invalid diff response")
    return {"passed": not differences, "differences": differences}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=Path)
    parser.add_argument("actual", type=Path)
    parser.add_argument("--cli", default="refract")
    parser.add_argument("--report", type=Path, default=Path("refract-report.json"))
    args = parser.parse_args()
    result = compare(args.baseline, args.actual, args.cli)
    args.report.write_text(json.dumps(result, indent=2) + "\n")
    print("Execution regression check " + ("passed" if result["passed"] else "failed"))
    raise SystemExit(0 if result["passed"] else 1)


if __name__ == "__main__":
    main()
