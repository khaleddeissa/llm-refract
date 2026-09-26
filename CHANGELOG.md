# Changelog

This file records user-visible changes by release. The current work targets **0.1.4** and
remains unreleased; add the actual release date when publishing its tag. Package versions stay at
0.1.4 while that release is in development.

## 0.1.4 — Unreleased

### Added

- Opt-in multi-provider instrumentation, LangChain/OTel integration, streaming usage and explicit pricing.
- Background export with sampling, bounded queues, durable retry spools and fail-open application behavior.
- Metrics, graph inspection, structured search, semantic graders, dataset evaluation and executable branches.
- Scoped API keys/RBAC, audit, retention, rate limits, encrypted storage and PostgreSQL support.
- Transactional delivery outbox and production deployment configuration.

### Fixed

- Replace complex HTTP delivery test state types with named aliases so strict Clippy checks pass.
- Rename the integration script to `platform_checks.py` so it cannot shadow Python's standard library.
- Add missing provider and OpenTelemetry/Langfuse guides and clarify tested integration boundaries.
- Check encrypted persisted payloads through SQLite so the production contract also handles live WAL writes.
- Preserve LangChain chat-message usage and tool calls when serializing nested Pydantic model results.

### Security

- Update `quinn-proto` to 0.11.15 for RUSTSEC-2026-0185 / GHSA-4w2j-m93h-cj5j.
- Reject unsafe TypeScript instrumentation paths before any method is patched; prevent prototype
  modification and restore original property descriptors, with regression coverage.
- Validate the intercepted Bedrock fixture URL by exact parsed scheme/authority instead of a prefix.

### Compatibility

`.rfr` and canonical run v1 remain readable. MCP adds tools and server-side pagination.
SDK context-managed export now defaults to fail-open; select `fail_open=False` / `failOpen: false` for
strict export failures. Storage migration 0002 upgrades scope/index layout; back up existing databases
and retain encryption keys. New functionality requires building this checkout until released.

## 0.1.0 — initial foundation

Initial local foundation: canonical model, portable artifacts, recorded playback, prefix forks, semantic
diff, SQLite API, manual Python/TypeScript SDKs, execution viewer, Docker and CI.
