# Security policy

`llm-refract` records AI executions and can connect to model providers, identity systems and delivery
endpoints. Treat recordings, service credentials and operator configuration as sensitive application
data. This policy explains how to report vulnerabilities and the security boundaries of the system.

## Supported versions

Security fixes target the **latest published release** and the active development branch. Older
releases do not receive separate backports. The project is pre-1.0 and has no long-term-support branch.
Use matching release versions of the service and SDKs, and review deployment changes before upgrading.

| Version                   | Security maintenance                                          |
| ------------------------- | ------------------------------------------------------------- |
| Latest published release  | Supported                                                     |
| Active development branch | Fixes developed here; use published artifacts for deployments |
| Older releases            | Upgrade to the latest release                                 |

Published artifacts are listed in [GitHub Releases](https://github.com/khaleddeissa/llm-refract/releases).
Do not infer that unpublished changes in this repository are already present in a package registry.

## Report a vulnerability privately

**Do not open a public issue or pull request containing an undisclosed vulnerability or credentials.**

- Preferred: [submit a private GitHub vulnerability report](https://github.com/khaleddeissa/llm-refract/security/advisories/new).
- If private reporting is unavailable, email **khaledayman012@gmail.com**.

Include the affected version/component, impact, prerequisites, reproduction steps and a minimal
proof of concept using synthetic data. For dependency findings, include the advisory identifier and
resolved version from the relevant lockfile. Redact API keys, tokens, prompts, personal information
and customer recordings; do not attach production database dumps.

We aim to acknowledge reports within **five business days**. After triage, we will coordinate a fix,
validation and disclosure with the reporter. Timing depends on the issue; acknowledgement is not a
remediation deadline. Reporter credit is offered unless anonymity is requested. No bounty program is
currently offered. Test only systems and data you own or have permission to assess; do not perform
destructive testing against other users or hosted services.

## Components covered

This policy covers code and distribution configuration maintained in this repository:

- Rust engine crates, storage/migrations, CLI, REST API and OTLP receivers.
- Python and npm/TypeScript SDKs, provider/framework adapters, exporters and artifact readers.
- The MCP server and bundled agent skills.
- The Inspector UI, OIDC sessions, SCIM provisioning and key-management APIs.
- Delivery integrations, the GitHub Action, container build and deployment examples.

Vulnerabilities in third-party providers or dependencies can be reported here when they affect Refract;
we may coordinate with the upstream maintainer. An external provider's availability, billing or
model output quality is governed by that provider.

## Trust and data boundaries

### Authentication and isolation

Local mode can run without authentication and is intended for a trusted development environment.
Do not expose it publicly. Production mode requires authentication, storage encryption and an explicit
assertion that HTTPS terminates at the ingress. These checks do not create certificates or verify
that the ingress was configured correctly.

Bearer identities select the organization, project, environment and role. Requests cannot assign their
own tenant scope. Managed API keys are stored as digests, and OIDC access tokens require signature,
issuer, audience, subject and expiry validation. OIDC subjects must be provisioned; token claims do
not grant administrator access. SCIM changes require a scoped administrator and derive roles from
operator-configured group mappings.

Persistent browser sessions use Secure, HttpOnly, SameSite cookies. Provider tokens and PKCE verifiers
stay encrypted on the server. Cookie-authenticated writes require the configured browser origin;
refresh uses a database lease across replicas. Logout and provisioning deactivation invalidate access.
API keys entered manually in the Inspector remain in tab memory. See
[identity and session configuration](docs/usage/service-controls.md).

Application queries enforce tenant scope. PostgreSQL deployments can additionally enable the supplied
forced row-security policies with a restricted runtime role. Migration/maintenance credentials remain
privileged and must be separated from runtime credentials. RLS does not make a compromised application
or database-owner account harmless. SQLite relies on application isolation and filesystem controls.

### Recordings and secrets

Recordings may contain prompts, responses, retrieved documents, tool inputs and personal information.
Built-in redaction and configurable rules reduce accidental disclosure; they are not a complete data
loss prevention system. Configure redaction before ingestion and inspect representative recordings
before sharing them. Redaction of new data does not sanitize earlier backups, artifacts or logs.

Configured AES-GCM encryption protects stored payloads and binds them to their scope/record identity.
Searchable metadata and authentication lookup fields remain readable to authorized database operators.
Use encrypted volumes/backups as well. Protect encryption keys separately, retain old keys while
historical data needs them, and test rotation and restoration. See [production operations](docs/production.md).

Readable `.rfr` files contain plaintext execution data. Their checksum detects content corruption;
it is **not** a publisher signature or proof that a recording is trustworthy. Validate artifacts,
apply access controls and use an encrypted transport/container when sharing sensitive recordings.

### Replay, models and outbound requests

Recorded playback returns captured outputs. Executable reruns require explicitly trusted application
handlers or operator-configured model profiles, plus the applicable live-call/policy approvals.
Artifact contents do not authorize arbitrary commands. Treat executor code, custom adapters, endpoint
configuration and provider credentials as trusted deployment configuration.

Automatic embeddings, text search and configured model grading may send text to an external provider.
Approve providers and scopes for your data-handling requirements, and restrict network egress. A model's
grading result is not a security decision or a guarantee of factual correctness. Provider SDKs and
custom handlers retain their own networking behavior; review their configuration separately.

MCP exposes read tools by default; write and live-model tools require explicit environment opt-ins.
These switches supplement service authentication and host approval policies. They do not grant extra
roles or make tool results trusted instructions. See [MCP mode](docs/usage/mcp.md).

### Delivery and audit

Webhook receivers should verify the signature over the exact body and commit a persistent receipt
before acknowledging delivery. Transport is at least once. Use delivery IDs/versions and the supplied
inbox helpers to handle retries and stale payloads; unrelated external side effects need their own
idempotency or transaction boundary. Keep spool directories private and on durable storage when using
durable SDK acceptance.

Use scoped audit exports alongside ingress and identity-provider logs. Avoid logging bearer headers,
OAuth codes, refresh tokens or sensitive query strings. Run retention, telemetry retention, audit
expiry and backup expiry are separate lifecycle concerns. Test them against your retention policy.

## Automated checks

| Check                                                         | Coverage                                                                                  | Trigger                                           |
| ------------------------------------------------------------- | ----------------------------------------------------------------------------------------- | ------------------------------------------------- |
| [CodeQL](.github/workflows/codeql.yml)                        | Python and JavaScript/TypeScript static analysis                                          | Pull requests and pushes to `main`                |
| [Dependency review](.github/workflows/dependency-review.yml)  | Dependency changes in a pull request                                                      | Pull requests                                     |
| [Dependency audits](.github/workflows/security.yml)           | Cargo advisories, npm runtime dependencies, Python dependencies including provider extras | Relevant pull requests/pushes and manual dispatch |
| [CI](.github/workflows/ci.yml)                                | Strict lint, types, tests, contracts, builds and recovery rehearsal                       | Relevant pull requests and pushes                 |
| [Docker/browser/PostgreSQL tests](.github/workflows/test.yml) | Service integration, UI behavior and database isolation                                   | Pull requests and manual dispatch                 |

[Dependabot](.github/dependabot.yml) checks monthly. Workflows have no scheduled cron triggers.
Dependency audits do not replace application review, and passing checks do not establish a security
certification. Repository owners should require applicable checks, enable private vulnerability
reporting and dependency alerts, and restrict release credentials. Configuration details are in the
[repository documentation](docs/README.md).
