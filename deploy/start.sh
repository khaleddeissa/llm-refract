#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
docker compose config --quiet
docker compose up --build -d --wait
exec docker compose ps
