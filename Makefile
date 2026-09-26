.PHONY: setup install hooks lint format test test-python test-js test-rust test-integration test-e2e build build-cli build-sdk docker-build docker-up docker-down generate docs security ci
setup install:
	uv sync --locked --all-packages --all-extras
	npm ci
hooks:
	uv run pre-commit install --install-hooks
	uv run pre-commit install --hook-type commit-msg
lint:
	cargo fmt --check
	cargo clippy --workspace --all-targets -- -D warnings
	uv run ruff check .
	uv run ruff format --check .
	uv run mypy
	npm run typecheck
	npm run format:check
format:
	cargo fmt --all
	uv run ruff check --fix .
	uv run ruff format .
	npm run format
test: test-rust test-python test-js
test-python:
	uv run pytest
test-js:
	npm test
test-rust:
	cargo test --workspace --locked
test-integration:
	python3 tests/integration/smoke.py
test-e2e:
	npx playwright test -c apps/viewer/playwright.config.ts
build:
	cargo build --workspace --locked
	npm run build
build-cli:
	cargo build --release --locked -p refract-cli
build-sdk:
	uv build --package llm-refract
	npm run build -w @llm-refract/sdk
docker-build:
	docker compose build
docker-up:
	docker compose up -d --build --wait
docker-down:
	docker compose down
generate:
	uv run python tools/dev/check-contracts.py
docs:
	python3 tools/dev/check-docs.py
security:
	cargo audit
	npm audit --omit=dev
ci: lint test build generate docs

typecheck-python:
	uv run mypy
migrate-info:
	cargo sqlx migrate info --source crates/refract-storage/migrations
migrate-add:
	cargo sqlx migrate add --source crates/refract-storage/migrations $(name)

test-contract:
	uv run python tests/contract/run.py

test-interfaces:
	uv run python tests/integration/interfaces.py
	node tests/integration/sdk.mjs
	uv run python tests/integration/platform_checks.py
