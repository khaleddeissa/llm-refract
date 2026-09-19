.PHONY: setup install hooks lint format test test-python test-js test-rust test-integration test-e2e build build-cli build-sdk docker-build docker-up docker-down generate docs security ci
setup install:
	uv sync --locked
	npm ci
hooks:
	uv run pre-commit install --install-hooks
	uv run pre-commit install --hook-type commit-msg
lint:
	cargo fmt --check
	cargo clippy --workspace --all-targets -- -D warnings
	uv run ruff check .
	uv run ruff format --check .
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
	python3 scripts/smoke-test.py
test-e2e:
	npx playwright test -c ui/playwright.config.ts
build:
	cargo build --workspace --locked
	npm run build
build-cli:
	cargo build --release --locked -p refract-cli
build-sdk:
	uv build --package refract
	npm run build -w @refract-ai/sdk
docker-build:
	docker compose build
docker-up:
	docker compose up -d --build --wait
docker-down:
	docker compose down
generate:
	uv run python scripts/check-contracts.py
docs:
	python3 scripts/check-docs.py
security:
	cargo audit
	npm audit --omit=dev
ci: lint test build generate docs
