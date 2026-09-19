# Local deployment

Run `sh deploy/start.sh` to validate Compose configuration, build the non-root image, and wait for readiness.
Use `docker compose down` to stop it while preserving recordings.
The deployed server and UI share port 8000, published only to loopback.
Remote staging/production targets, PostgreSQL/S3 and Kubernetes are not configured.
