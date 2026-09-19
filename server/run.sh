#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
exec cargo run --locked -p refract-server
