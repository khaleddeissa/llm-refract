"""Compare a baseline and freshly recorded artifact using the shared Rust engine."""

import argparse
import json
import os
import subprocess
from pathlib import Path


def compare(baseline: Path, actual: Path, cli: str, options: dict | None = None) -> dict:
    # Validate separately so an invalid artifact cannot be confused with a semantic difference.
    for path in (baseline, actual):
        subprocess.run([cli, "validate", str(path.resolve())], check=True, capture_output=True)
    command = [cli, "diff", str(baseline.resolve()), str(actual.resolve())]
    if options is not None:
        allowed = {
            "similarity_threshold": "threshold",
            "max_cost_increase_percent": "max-cost-increase-percent",
            "max_latency_increase_percent": "max-latency-increase-percent",
            "max_token_increase_percent": "max-token-increase-percent",
        }
        if not isinstance(options, dict) or set(options) - set(allowed):
            raise ValueError("unsupported comparison options")
        command.append("--semantic")
        for key, value in options.items():
            if not isinstance(value, (int, float)) or isinstance(value, bool):
                raise ValueError("comparison thresholds must be numbers")
            command.extend(["--" + allowed[key], str(value)])
    result = subprocess.run(
        command,
        check=False,
        text=True,
        capture_output=True,
    )
    if result.returncode not in (0, 1):
        raise RuntimeError(result.stderr)
    differences = json.loads(result.stdout)
    if options is not None:
        if not isinstance(differences, dict) or not isinstance(differences.get("passed"), bool):
            raise TypeError("invalid semantic diff response")
        return differences
    if not isinstance(differences, list):
        raise TypeError("invalid diff response")
    return {"passed": not differences, "differences": differences}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=Path)
    parser.add_argument("actual", type=Path)
    parser.add_argument("--cli", default="refract")
    parser.add_argument("--report", type=Path, default=Path("refract-report.json"))
    parser.add_argument(
        "--options", type=Path, default=os.environ.get("REFRACT_COMPARE_OPTIONS") or None
    )
    args = parser.parse_args()
    options = json.loads(args.options.read_text()) if args.options else None
    result = compare(args.baseline, args.actual, args.cli, options)
    args.report.write_text(json.dumps(result, indent=2) + "\n")
    print("Execution regression check " + ("passed" if result["passed"] else "failed"))
    raise SystemExit(0 if result["passed"] else 1)


if __name__ == "__main__":
    main()
