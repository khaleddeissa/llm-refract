# SCIM provisioning

Configure OIDC verification and a scoped administrator key as described in
[service controls](../../docs/usage/service-controls.md#scim-users-and-groups).
This example creates a synthetic subject, then deactivates it. It does not contact an identity
provider, issue a token or provision a real employee. Use a disposable project; each execution
requires a fresh directory or a different userName/externalId.

```bash
export REFRACT_SERVER_URL=http://127.0.0.1:8000
# Supply the scoped admin key through REFRACT_API_KEY when authentication is enabled.
uv run python examples/identity/provision.py
```

Map your identity provider's externalId to its immutable OIDC subject. Optional group mappings grant
reader/writer/admin access; user-provided roles never grant privileges. The Rust server tests exercise
PATCH rollback, group membership removal, tenant conflicts, encrypted sessions and revocation.
