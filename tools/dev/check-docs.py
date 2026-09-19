"""Check relative Markdown links in maintained documentation."""

import re
from pathlib import Path

root = Path(__file__).resolve().parents[2]
paths = [
    p
    for base in [
        root,
        root / "docs",
        root / "spec",
        root / "examples",
        root / "packages",
        root / "skills",
    ]
    for p in (base.glob("*.md") if base == root else base.rglob("*.md"))
    if not any(part in {"node_modules", "dist", "__pycache__"} for part in p.parts)
]

failures = []
for path in paths:
    for target in re.findall(r"\[[^\]]*\]\(([^)]+)\)", path.read_text()):
        if "://" in target or target.startswith("#"):
            continue
        if not (path.parent / target.split("#")[0]).exists():
            failures.append(f"{path.relative_to(root)}: {target}")
if failures:
    raise SystemExit("Broken links:\n" + "\n".join(failures))
print(f"Checked links in {len(paths)} Markdown files")
