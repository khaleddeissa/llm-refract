# Production use and current boundaries

Distinguish **instrumenting application code** from **operating the Refract service**.
Manual SDK recording works with real provider/framework outputs after the application converts them
to JSON-compatible values. Local files avoid a collector dependency; remote recording requires a
reachable service. Neither mode guarantees reproducing non-deterministic model/tool behavior.

SDK exports happen at run completion. Python performs synchronous writes/HTTP; Node awaits export.
An export failure can fail an otherwise successful call. Capture only data your policy permits and
choose your application-level failure strategy deliberately. There is no batching, sampling, background
queue, durable retry transport, universal PII detector or at-rest encryption yet.

The current server is a **local/single-workspace developer service**, not a production-ready public
multi-tenant platform. It has no authentication/authorization, tenancy, audit trail, retention controls
or rate limits. TLS termination and network isolation alone do not implement those missing features.
Compose therefore publishes only to loopback by default. MCP's optional write flag is not access control.

For a controlled internal evaluation, keep access restricted, back up SQLite consistently, bound the
recording volume, review redaction and validate upgrades on copied data. A deployment owner must design
and implement the missing controls before public or sensitive shared production hosting.
Do not treat the candidate release/deployment workflows as production rollout automation.

Live replay, tool invocation, provider substitution and checkpoint restoration are intentionally absent.
Recorded playback cannot send an email, charge a card or call an inference provider. Prefix forks are
new data snapshots, not executable continuations. The [roadmap](roadmap.md) describes those future steps.
