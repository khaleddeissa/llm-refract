# What is an `.rfr` file?

An `.rfr` is a portable execution recording, not executable code. It stores run metadata and ordered
canonical events: model calls, tools, retrieval, state and errors. No provider account is required to
inspect it. It can be recorded by an SDK, exported by the API and consumed by the CLI or a library.

## Readable text profile

New writers produce UTF-8 bytes with:

1. A compact JSON header on the first line: format `refract.artifact.v1`, encoding `json`, and SHA-256.
2. A formatted canonical JSON execution document on the remaining lines, ending with LF.

Open [the demo recording](../../examples/artifacts/demo.rfr) in any text editor. The header and body
are two JSON values separated by the first newline; the whole file is not a single JSON document.
The digest covers the **exact body bytes**, including whitespace and the final newline. Editing the body
invalidates its checksum. Use `unpack` to obtain editable JSON, then `pack` to produce a fresh artifact.
The header records a container profile; the body retains `refract.execution.v1` as its schema version.

```bash
refract inspect examples/artifacts/demo.rfr
refract unpack examples/artifacts/demo.rfr -o .examples/editable.json
# Edit the canonical JSON if needed, then create a new recording:
refract pack .examples/editable.json -o .examples/updated.rfr
refract validate .examples/updated.rfr
```

Python: `from refract.artifact import pack, unpack`; Node: `import {pack, unpack} from '@refract-ai/sdk'`;
Rust: `refract_artifact::{pack, unpack}`. SDK readers verify the text profile/checksum; Rust performs
full canonical validation and also handles legacy archives. Use the Rust API/CLI when accepting untrusted
recordings and requiring the full graph-validation boundary.

## Existing binary recordings

The original writer used ZIP containers (`PK` magic bytes), hence special characters in editors.
They were valid archives, not corrupted text. Rust continues to read and validate those recordings.
Convert one without overwriting it:

```bash
refract pack old-binary.rfr -o .examples/readable.rfr
```

Python/Node readers support the new text profile; convert legacy ZIP files with Rust first.
The legacy fixture remains in `tests/fixtures/simple-run/python.rfr` to test compatibility.

## Integrity and data handling

Payloads are limited to 16 MiB. Checksums detect changed bytes, not provenance or malicious rewriting.
There is no encryption or signature. Key-based redaction does not scrub all free-text PII. Review
recordings before sharing or uploading. See the [normative format specification](../../spec/artifact/rfr-v1.md).
