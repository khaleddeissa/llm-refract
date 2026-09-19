# Refract artifact formats

## Text profile: `refract.artifact.v1`

Writers emit UTF-8 without BOM. The first LF terminates a JSON object containing:
`format: "refract.artifact.v1"`, `encoding: "json"`, `sha256: <lowercase 64-character hex>`.
The remaining bytes are the canonical `refract.execution.v1` JSON run, including events, pretty printed
and terminated with LF. SHA-256 covers those exact payload bytes. The header is not covered by a
signature; readers validate its profile fields and digest. Maximum payload is 16 MiB; header is at most
4096 bytes. Writers are deterministic for identical input within an implementation; cross-language
JSON formatting may differ while remaining interoperable. Editors must not rewrite newline encoding
without repacking. The file is a header plus JSON body, not one standalone JSON document.

## Legacy ZIP profile

Rust detects the `PK` prefix and reads the original ZIP container. Exactly three distinct entries:
`manifest.json`, `execution.json` with `events: []`, and `events.jsonl`. The manifest has
`spec_version: "refract.execution.v1"` and a `files` map with SHA-256 hashes for the other two entries.
Event lines end with LF. Writers used stored ZIP entries with fixed 1980-01-01 timestamps.
Readers reject unexpected/duplicate paths, checksum mismatches and oversized entries; no archive
member is extracted to arbitrary filesystem locations. Total expanded entries may not exceed 16 MiB
plus 4 KiB manifest overhead; archive bytes are capped at 16 MiB plus 64 KiB.

Default writers now emit text. `refract pack LEGACY.rfr -o NEW.rfr` converts legacy archives into the
text profile without rewriting the input. The legacy execution schema remains unchanged.
Both profiles carry data only. External blobs, signatures and encrypted containers are not implemented.
