# Deployment profiles

- `docker-compose.yml` at the repository root: local non-root service and Inspector, SQLite persistent
  volume, loopback port 8000. `sh deploy/start.sh` validates configuration, builds and waits for readiness.
- `production/docker-compose.yml`: PostgreSQL 17, secret files, isolated service network and Caddy HTTPS
  ingress. Supply your hostname/secrets and follow [production operation](../docs/production.md).
- `production/Caddyfile`: HTTPS headers and HTTP/2 upstream forwarding for REST/UI and native OTLP gRPC.
- `production/row-security.sql`: optional forced tenant policies for a restricted PostgreSQL runtime role.
- `docker/entrypoint.sh`: forwards the container command/signals to the unprivileged process.

Delivery endpoints, OIDC and workload identity are configured through service environment/secret files;
allow only their required outbound destinations. Compose does not provision cloud resources or issue
provider credentials. See [backup and recovery](../docs/usage/recovery.md) and
[production operation](../docs/production.md).
