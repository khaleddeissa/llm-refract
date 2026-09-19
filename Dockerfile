FROM node:24-bookworm-slim AS ui
WORKDIR /build
COPY package.json package-lock.json tsconfig.json ./
COPY packages/typescript packages/typescript
COPY apps/viewer apps/viewer
COPY tests/fixtures tests/fixtures
RUN npm ci && npm run build

FROM rust:1.97-bookworm AS rust
WORKDIR /build
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates crates
COPY tests/fixtures tests/fixtures
RUN cargo build --release --locked -p refract-cli -p refract-server

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/*     && groupadd --gid 10001 refract && useradd --uid 10001 --gid refract --no-create-home refract     && mkdir -p /data && chown refract:refract /data
COPY --from=rust /build/target/release/refract /usr/local/bin/refract
COPY --from=rust /build/target/release/refract-server /usr/local/bin/refract-server
COPY --from=ui /build/apps/viewer/dist /app/ui
COPY --chmod=755 deploy/docker/entrypoint.sh /usr/local/bin/entrypoint.sh
ENV REFRACT_BIND=0.0.0.0:8000 REFRACT_DATABASE_URL=sqlite:///data/refract.db REFRACT_UI_DIR=/app/ui
USER 10001:10001
WORKDIR /data
EXPOSE 8000
HEALTHCHECK --interval=10s --timeout=3s --start-period=10s CMD curl -fsS http://127.0.0.1:8000/v1/ready || exit 1
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
CMD ["refract-server"]
