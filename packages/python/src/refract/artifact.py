"""Portable v1 ZIP writer. Checksums cover bytes, not language-specific JSON ordering."""

import hashlib
import io
import json
import zipfile


def pack(run: dict) -> bytes:
    execution = {**run, "events": []}
    encode = lambda value: json.dumps(value, separators=(",", ":"), allow_nan=False).encode()
    files = {
        "execution.json": encode(execution),
        "events.jsonl": b"".join(encode(e) + b"\n" for e in run["events"]),
    }
    if sum(map(len, files.values())) > 16 * 1024 * 1024:
        raise ValueError("artifact exceeds size limit")
    manifest = {
        "spec_version": "refract.execution.v1",
        "files": {k: hashlib.sha256(v).hexdigest() for k, v in files.items()},
    }
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_STORED) as archive:
        for name, body in {"manifest.json": encode(manifest), **files}.items():
            info = zipfile.ZipInfo(name, (1980, 1, 1, 0, 0, 0))
            info.external_attr = 0o100600 << 16
            archive.writestr(info, body)
    return output.getvalue()
