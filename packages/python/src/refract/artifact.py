"""Readable, checksummed .rfr text artifacts. Rust also reads legacy ZIP recordings."""

import hashlib
import json
from typing import Any

MAX_BYTES = 16 * 1024 * 1024


def pack(run: dict[str, Any]) -> bytes:
    payload = (json.dumps(run, indent=2, ensure_ascii=False, allow_nan=False) + "\n").encode()
    if len(payload) > MAX_BYTES:
        raise ValueError("artifact exceeds size limit")
    header = {
        "format": "refract.artifact.v1",
        "encoding": "json",
        "sha256": hashlib.sha256(payload).hexdigest(),
    }
    return json.dumps(header, separators=(",", ":")).encode() + b"\n" + payload


def unpack(data: bytes) -> dict[str, Any]:
    if len(data) > MAX_BYTES + 4097:
        raise ValueError("artifact exceeds size limit")
    header_bytes, separator, payload = data.partition(b"\n")
    if not separator or len(header_bytes) > 4096 or len(payload) > MAX_BYTES:
        raise ValueError("invalid artifact size/header")
    header = json.loads(header_bytes)
    if header.get("format") != "refract.artifact.v1" or header.get("encoding") != "json":
        raise ValueError("unsupported artifact profile; use the Rust CLI for legacy ZIP files")
    if header.get("sha256") != hashlib.sha256(payload).hexdigest():
        raise ValueError("checksum mismatch")
    run = json.loads(payload)
    if not isinstance(run, dict) or run.get("spec_version") != "refract.execution.v1":
        raise ValueError("unsupported execution version")
    return run
