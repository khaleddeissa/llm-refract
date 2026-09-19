# Security

This foundation is for local development. Keep the service bound to loopback. Authentication,
authorization, encrypted artifacts, retention and production rate limits are not implemented.

Recording applies recursive key-based redaction for password, secret, token, API key, authorization,
cookie and email keys. This is a baseline, not a PII detector: free text, event names and arbitrary values
can contain secrets. Token-named usage fields may also be redacted. Review artifacts before sharing.
Checksums are not digital signatures. Archive reads are bounded and never extract paths.

Exact playback never executes tools. `BLOCKED` events reject playback. Other replay policies describe
future executor capabilities; they never authorize live execution in this version. Forks only copy data.

Report vulnerabilities privately using this repository's GitHub Security Advisory channel when enabled.
Until a private reporting channel is configured by the maintainers, do not post sensitive examples in
public issues. No security contact address or production support promise has been established yet.
